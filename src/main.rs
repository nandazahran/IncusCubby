mod incus;
mod launcher;
mod settings;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{Model, ModelRc, SharedString, VecModel, Weak};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;

use crate::incus::community::{
    CommunityApp, CommunityAppSpec, CommunityRepo, build_container_runner_script,
    curated_community_apps, fetch_community_app_spec, fetch_community_catalog, fetch_install_func,
};
use crate::incus::packages::{install_command, upgrade_command};
use crate::incus::remote_images::{default_preset_aliases, fetch_remote_images};
use crate::incus::{
    BridgeNat, CreateInstanceParams, ExecOutput, ImageSummary, IncusClient, IncusError,
    InstanceSummary, NestingMode, NetworkSummary, ProfileSummary, StoragePoolSummary,
    StorageVolumeSummary,
};
use crate::settings::{AppSettings, available_ides, available_terminals};

slint::include_modules!();

enum ProfileSaveMode {
    Create,
    Update,
}

enum BackendCommand {
    Refresh,
    Create {
        name: String,
        instance_type: String,
        image: String,
        memory: String,
        cpu: String,
        workspace: String,
        profiles: Vec<String>,
        cloud_init: String,
        nesting: NestingMode,
    },
    Clone {
        source: String,
        new_name: String,
    },
    SetState {
        name: String,
        action: String,
    },
    UpdateLimits {
        name: String,
        cpu: String,
        memory: String,
    },
    DeleteInstance {
        name: String,
    },
    AddDiskMount {
        instance: String,
        dev_name: String,
        host_path: String,
        container_path: String,
    },
    AttachCustomVolume {
        instance: String,
        dev_name: String,
        pool: String,
        volume: String,
        container_path: String,
    },
    AddProxyDevice {
        instance: String,
        dev_name: String,
        host_port: u16,
        container_port: u16,
    },
    RemoveDevice {
        instance: String,
        dev_name: String,
    },
    CreateSnapshot {
        instance: String,
        snapshot: String,
    },
    RestoreSnapshot {
        instance: String,
        snapshot: String,
    },
    DeleteSnapshot {
        instance: String,
        snapshot: String,
    },
    ExecCommand {
        instance: String,
        label: String,
        command: Vec<String>,
    },
    RefreshInstanceLogs {
        instance: String,
    },
    ReadInstanceLog {
        instance: String,
        log_name: String,
    },
    PushInstanceFile {
        instance: String,
        host_path: String,
        container_path: String,
    },
    PullInstanceFile {
        instance: String,
        container_path: String,
        host_path: String,
    },
    RefreshInfra,
    CreateStorageVolume {
        pool: String,
        name: String,
    },
    DeleteStorageVolume {
        pool: String,
        name: String,
    },
    CreateNetwork {
        name: String,
        nat: BridgeNat,
    },
    DeleteNetwork {
        name: String,
    },
    SaveProfile {
        name: String,
        description: String,
        cpu: String,
        memory: String,
        mode: ProfileSaveMode,
    },
    DeleteProfile {
        name: String,
    },
    FetchRemoteImages,
    RefreshLocalImages,
    DeleteImage {
        fingerprint: String,
    },
    FetchCommunityCatalog,
    LoadCommunityAppSpec {
        slug: String,
    },
    DeployCommunityApp {
        slug: String,
        instance_name: String,
        image_alias: String,
        cpu: String,
        memory: String,
        nesting: NestingMode,
    },
    RunCommunityInInstance {
        slug: String,
        instance_name: String,
        image_alias: String,
    },
}

impl BackendCommand {
    fn busy_label(&self) -> String {
        match self {
            Self::Refresh => "Refreshing sandboxes...".to_string(),
            Self::Create { name, image, .. } => format!("Creating {name} ({image})..."),
            Self::Clone { source, new_name } => format!("Cloning {source} -> {new_name}..."),
            Self::SetState { name, action } => format!("Running '{action}' on {name}..."),
            Self::UpdateLimits { name, .. } => format!("Updating limits on {name}..."),
            Self::DeleteInstance { name } => format!("Deleting {name}..."),
            Self::AddDiskMount {
                instance, dev_name, ..
            } => format!("Adding mount '{dev_name}' on {instance}..."),
            Self::AttachCustomVolume {
                instance, volume, ..
            } => format!("Attaching volume '{volume}' on {instance}..."),
            Self::AddProxyDevice {
                instance,
                host_port,
                container_port,
                ..
            } => format!("Forwarding {host_port}->{container_port} on {instance}..."),
            Self::RemoveDevice { instance, dev_name } => {
                format!("Removing '{dev_name}' on {instance}...")
            }
            Self::CreateSnapshot { instance, snapshot } => {
                format!("Creating snapshot {instance}/{snapshot}...")
            }
            Self::RestoreSnapshot { instance, snapshot } => {
                format!("Restoring snapshot {instance}/{snapshot}...")
            }
            Self::DeleteSnapshot { instance, snapshot } => {
                format!("Deleting snapshot {instance}/{snapshot}...")
            }
            Self::ExecCommand {
                instance, label, ..
            } => format!("{label} in {instance}..."),
            Self::RefreshInstanceLogs { instance } => format!("Listing logs for {instance}..."),
            Self::ReadInstanceLog { instance, log_name } => {
                format!("Reading {log_name} for {instance}...")
            }
            Self::PushInstanceFile { instance, .. } => format!("Pushing file to {instance}..."),
            Self::PullInstanceFile { instance, .. } => format!("Pulling file from {instance}..."),
            Self::RefreshInfra => "Refreshing storage & networks...".to_string(),
            Self::CreateStorageVolume { pool, name } => format!("Creating volume {pool}/{name}..."),
            Self::DeleteStorageVolume { pool, name } => format!("Deleting volume {pool}/{name}..."),
            Self::CreateNetwork { name, .. } => format!("Creating network {name}..."),
            Self::DeleteNetwork { name } => format!("Deleting network {name}..."),
            Self::SaveProfile { name, .. } => format!("Saving profile {name}..."),
            Self::DeleteProfile { name } => format!("Deleting profile {name}..."),
            Self::FetchRemoteImages => "Fetching remote image aliases...".to_string(),
            Self::RefreshLocalImages => "Refreshing cached images...".to_string(),
            Self::DeleteImage { fingerprint } => {
                let short: String = fingerprint.chars().take(12).collect();
                format!("Deleting cached image {short}...")
            }
            Self::FetchCommunityCatalog => "Loading community-scripts/Incus catalog...".to_string(),
            Self::LoadCommunityAppSpec { slug } => format!("Loading '{slug}' script..."),
            Self::DeployCommunityApp {
                slug,
                instance_name,
                ..
            } => format!("Deploying community app '{slug}' into {instance_name}..."),
            Self::RunCommunityInInstance {
                slug,
                instance_name,
                ..
            } => format!("Running '{slug}' script inside {instance_name}..."),
        }
    }
}

struct InfraSnapshot {
    pools: Vec<StoragePoolSummary>,
    volumes: Vec<StorageVolumeSummary>,
    networks: Vec<NetworkSummary>,
}

fn main() -> anyhow::Result<()> {
    let ui = AppWindow::new()?;
    let app_settings = AppSettings::load();
    let community_catalog = Arc::new(Mutex::new(curated_community_apps()));
    init_settings_ui(&ui, &app_settings, &community_catalog);

    let (tx, rx) = mpsc::unbounded_channel::<BackendCommand>();
    spawn_backend_worker(ui.as_weak(), rx, Arc::clone(&community_catalog));
    wire_ui_callbacks(&ui, &tx, app_settings, &community_catalog);

    let _ = tx.send(BackendCommand::Refresh);
    ui.run()?;
    Ok(())
}

fn init_settings_ui(
    ui: &AppWindow,
    app_settings: &AppSettings,
    community_catalog: &Arc<Mutex<Vec<CommunityApp>>>,
) {
    let terms = available_terminals();
    let ides = available_ides();

    let term_pref = if !app_settings.preferred_terminal.is_empty()
        && terms.contains(&app_settings.preferred_terminal)
    {
        app_settings.preferred_terminal.clone()
    } else {
        "auto".to_string()
    };
    let ide_pref = if app_settings.preferred_ide.trim().is_empty() {
        "auto".to_string()
    } else {
        app_settings.preferred_ide.clone()
    };

    ui.set_available_terminals(to_shared_string_model(terms));
    ui.set_available_ides(to_shared_string_model(ides));
    ui.set_preferred_terminal(term_pref.into());
    ui.set_preferred_ide(ide_pref.into());
    ui.set_remote_aliases(to_shared_string_model(default_preset_aliases()));

    let apps = community_catalog
        .lock()
        .expect("community catalog mutex poisoned")
        .clone();
    apply_community_apps(ui, &apps, "");
    init_about_ui(ui);
}

fn init_about_ui(ui: &AppWindow) {
    let version = env!("CARGO_PKG_VERSION");
    let license = env!("CARGO_PKG_LICENSE");
    let repo = env!("CARGO_PKG_REPOSITORY");
    let desc = env!("CARGO_PKG_DESCRIPTION");

    ui.set_about_header(format!("IncusCubby v{version} ({license})").into());
    ui.set_about_description(format!("{desc}\nRepository: {repo}").into());
    ui.set_about_notices(
        "IncusCubby is an independent desktop client and is not affiliated with or endorsed by LinuxContainers, Incus, or community-scripts.\n\
         - Incus daemon & REST API: https://linuxcontainers.org/incus/ (Apache-2.0)\n\
         - Container & VM base images: https://images.linuxcontainers.org (LinuxContainers Simplestreams)\n\
         - Community Scripts catalog: https://community-scripts.org (MIT; third-party scripts executed inside containers—review previews before deploying)\n\
         - Slint UI toolkit: https://slint.dev (GPL-3.0)"
            .into(),
    );
    ui.set_about_license_text(include_str!("../LICENSE").into());
}

fn spawn_backend_worker(
    ui_weak: Weak<AppWindow>,
    mut rx: mpsc::UnboundedReceiver<BackendCommand>,
    community_catalog: Arc<Mutex<Vec<CommunityApp>>>,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to initialize Tokio runtime");

        rt.block_on(async move {
            let client = IncusClient::default_socket();
            let mut poll_tick = tokio::time::interval(Duration::from_secs(3));
            poll_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
            poll_tick.tick().await;

            let mut last_instances: Option<Vec<InstanceSummary>> = None;
            let mut last_socket_err = false;
            let mut cached_spec: Option<CommunityAppSpec> = None;

            {
                let weak_startup = ui_weak.clone();
                let catalog_startup = Arc::clone(&community_catalog);
                tokio::spawn(async move {
                    if let Ok(apps) = fetch_community_catalog().await {
                        catalog_startup
                            .lock()
                            .expect("community catalog mutex poisoned")
                            .clone_from(&apps);
                        let _ = weak_startup.upgrade_in_event_loop(move |ui| {
                            let query = ui.get_community_search_query().to_string();
                            apply_community_apps(&ui, &apps, &query);
                        });
                    }
                });
            }

            loop {
                tokio::select! {
                    maybe_cmd = rx.recv() => {
                        let Some(cmd) = maybe_cmd else { break };
                        let busy_msg = cmd.busy_label();
                        let weak_busy = ui_weak.clone();
                        let _ = weak_busy.upgrade_in_event_loop(move |ui| {
                            ui.set_busy(true);
                            ui.set_status_message(busy_msg.into());
                        });

                        handle_command(
                            &client,
                            cmd,
                            &ui_weak,
                            &mut last_instances,
                            &mut last_socket_err,
                            &community_catalog,
                            &mut cached_spec,
                        )
                        .await;
                    }
                    _ = poll_tick.tick() => {
                        match client.list_instances().await {
                            Ok(instances) => {
                                if last_socket_err || last_instances.as_ref() != Some(&instances) {
                                    last_instances = Some(instances.clone());
                                    last_socket_err = false;
                                    let weak_poll = ui_weak.clone();
                                    let _ = weak_poll.upgrade_in_event_loop(move |ui| {
                                        if !ui.get_busy() {
                                            apply_instances(&ui, instances);
                                        }
                                    });
                                }
                            }
                            Err(err) => {
                                let is_sock = err.socket_warning().is_some();
                                if is_sock && !last_socket_err {
                                    last_socket_err = true;
                                    last_instances = None;
                                    let weak_err = ui_weak.clone();
                                    let _ = weak_err.upgrade_in_event_loop(move |ui| {
                                        if !ui.get_busy() {
                                            apply_error(&ui, &err);
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }
        });
    });
}

async fn handle_command(
    client: &IncusClient,
    cmd: BackendCommand,
    ui_weak: &Weak<AppWindow>,
    last_instances: &mut Option<Vec<InstanceSummary>>,
    last_socket_err: &mut bool,
    community_catalog: &Arc<Mutex<Vec<CommunityApp>>>,
    cached_spec: &mut Option<CommunityAppSpec>,
) {
    match cmd {
        BackendCommand::FetchCommunityCatalog => {
            let res = fetch_community_catalog().await;
            if let Ok(ref apps) = res {
                community_catalog
                    .lock()
                    .expect("community catalog mutex poisoned")
                    .clone_from(apps);
            }
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(apps) => {
                        let count = apps.len();
                        let query = ui.get_community_search_query().to_string();
                        apply_community_apps(&ui, &apps, &query);
                        ui.set_status_message(format!("Loaded {count} community apps").into());
                    }
                    Err(err) => {
                        ui.set_status_message(format!("Community catalog failed: {err}").into());
                    }
                }
            });
            return;
        }
        BackendCommand::LoadCommunityAppSpec { slug } => {
            let res = load_or_cached_spec(&slug, community_catalog, cached_spec).await;
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(spec) => {
                        ui.set_community_selected_slug(spec.slug.into());
                        ui.set_community_app_name(spec.app_name.into());
                        ui.set_community_description(spec.description.into());
                        ui.set_community_tags(spec.tags.into());
                        ui.set_community_image_alias(spec.image_alias.into());
                        ui.set_community_cpu(spec.cpu_limit.into());
                        ui.set_community_memory(spec.memory_limit.into());
                        ui.set_community_nesting(spec.nesting);
                        ui.set_community_install_script(spec.install_script.into());
                        ui.set_community_output("".into());
                        ui.set_status_message(format!("Loaded script '{slug}'").into());
                    }
                    Err(err) => {
                        ui.set_status_message(format!("Failed loading '{slug}': {err}").into());
                    }
                }
            });
            return;
        }
        BackendCommand::DeployCommunityApp {
            slug,
            instance_name,
            image_alias,
            cpu,
            memory,
            nesting,
        } => {
            let deploy_res = deploy_community_sandbox(
                client,
                DeployCommunityRequest {
                    slug: &slug,
                    instance_name: &instance_name,
                    image_alias: &image_alias,
                    cpu: &cpu,
                    memory: &memory,
                    nesting,
                },
                community_catalog,
                cached_spec,
            )
            .await;
            let list_res = client.list_instances().await;
            if let Ok(ref instances) = list_res {
                *last_instances = Some(instances.clone());
                *last_socket_err = false;
            }
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                if let Ok(instances) = list_res {
                    apply_instances(&ui, instances);
                }
                match deploy_res {
                    Ok(out) => {
                        ui.set_community_output(format_exec_output(&out).into());
                        ui.set_status_message(
                            format!(
                                "Deployed '{slug}' -> {instance_name} (exit {})",
                                out.exit_code
                            )
                            .into(),
                        );
                    }
                    Err(err) => {
                        ui.set_community_output(format!("Deploy error:\n{err:#}").into());
                        ui.set_status_message(format!("Deploy failed: {err}").into());
                    }
                }
            });
            return;
        }
        BackendCommand::RunCommunityInInstance {
            slug,
            instance_name,
            image_alias,
        } => {
            let run_res = async {
                let spec = load_or_cached_spec(&slug, community_catalog, cached_spec).await?;
                execute_community_install(client, &spec, &instance_name, &image_alias).await
            }
            .await;
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match run_res {
                    Ok(out) => {
                        ui.set_community_output(format_exec_output(&out).into());
                        ui.set_status_message(
                            format!("Ran '{slug}' in {instance_name} (exit {})", out.exit_code)
                                .into(),
                        );
                    }
                    Err(err) => {
                        ui.set_community_output(format!("Run error:\n{err:#}").into());
                        ui.set_status_message(format!("Community run failed: {err}").into());
                    }
                }
            });
            return;
        }
        BackendCommand::FetchRemoteImages => {
            let res = fetch_remote_images().await;
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(imgs) if !imgs.is_empty() => {
                        let aliases: Vec<String> = imgs.into_iter().map(|i| i.alias).collect();
                        let count = aliases.len();
                        ui.set_remote_aliases(to_shared_string_model(aliases));
                        ui.set_status_message(
                            format!("Loaded {count} remote image aliases").into(),
                        );
                    }
                    Ok(_) => ui.set_status_message("No matching remote images found".into()),
                    Err(err) => ui.set_status_message(format!("Remote fetch failed: {err}").into()),
                }
            });
            return;
        }
        BackendCommand::RefreshLocalImages => {
            let res = client.list_images().await;
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(images) => apply_images(&ui, images),
                    Err(err) => apply_error(&ui, &err),
                }
            });
            return;
        }
        BackendCommand::DeleteImage { fingerprint } => {
            let op = client.delete_image(&fingerprint).await;
            let imgs = match op {
                Ok(()) => client.list_images().await,
                Err(err) => Err(err),
            };
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match imgs {
                    Ok(images) => apply_images(&ui, images),
                    Err(err) => apply_error(&ui, &err),
                }
            });
            return;
        }
        BackendCommand::ExecCommand {
            instance,
            label,
            command,
        } => {
            let res = client.exec_command(&instance, &command).await;
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(out) => apply_exec_output(&ui, &label, &out),
                    Err(err) => apply_error(&ui, &err),
                }
            });
            return;
        }
        BackendCommand::RefreshInstanceLogs { instance } => {
            let res = client.list_instance_logs(&instance).await;
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(logs) => {
                        let count = logs.len();
                        let first = logs.first().cloned().unwrap_or_default();
                        ui.set_instance_logs(to_shared_string_model(logs));
                        ui.set_selected_log(first.into());
                        ui.set_status_message(format!("{count} log file(s) for {instance}").into());
                    }
                    Err(err) => apply_error(&ui, &err),
                }
            });
            return;
        }
        BackendCommand::ReadInstanceLog { instance, log_name } => {
            let res = client.read_instance_log(&instance, &log_name).await;
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(text) => {
                        ui.set_log_content(text.into());
                        ui.set_status_message(format!("Loaded {log_name}").into());
                    }
                    Err(err) => apply_error(&ui, &err),
                }
            });
            return;
        }
        BackendCommand::PushInstanceFile {
            instance,
            host_path,
            container_path,
        } => {
            let res = push_file_from_host(client, &instance, &host_path, &container_path).await;
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(msg) | Err(msg) => ui.set_status_message(msg.into()),
                }
            });
            return;
        }
        BackendCommand::PullInstanceFile {
            instance,
            container_path,
            host_path,
        } => {
            let res = pull_file_to_host(client, &instance, &container_path, &host_path).await;
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(msg) | Err(msg) => ui.set_status_message(msg.into()),
                }
            });
            return;
        }
        BackendCommand::RefreshInfra
        | BackendCommand::CreateStorageVolume { .. }
        | BackendCommand::DeleteStorageVolume { .. }
        | BackendCommand::CreateNetwork { .. }
        | BackendCommand::DeleteNetwork { .. } => {
            let op_res = execute_infra_command(client, cmd).await;
            let infra_res = match op_res {
                Ok(()) => fetch_infra_snapshot(client).await,
                Err(err) => Err(err),
            };
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match infra_res {
                    Ok(snap) => {
                        apply_infra(&ui, snap);
                        ui.set_status_message("Storage & networks updated".into());
                    }
                    Err(err) => apply_error(&ui, &err),
                }
            });
            return;
        }
        BackendCommand::SaveProfile {
            name,
            description,
            cpu,
            memory,
            mode,
        } => {
            let op_res = match mode {
                ProfileSaveMode::Create => {
                    client
                        .create_profile(&name, &description, &cpu, &memory)
                        .await
                }
                ProfileSaveMode::Update => {
                    client
                        .update_profile(&name, &description, &cpu, &memory)
                        .await
                }
            };
            let prof_res = match op_res {
                Ok(()) => client.list_profiles().await,
                Err(err) => Err(err),
            };
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match prof_res {
                    Ok(profiles) => {
                        apply_profiles(&ui, profiles);
                        ui.set_status_message(format!("Saved profile '{name}'").into());
                    }
                    Err(err) => apply_error(&ui, &err),
                }
            });
            return;
        }
        BackendCommand::DeleteProfile { name } => {
            let op_res = client.delete_profile(&name).await;
            let prof_res = match op_res {
                Ok(()) => client.list_profiles().await,
                Err(err) => Err(err),
            };
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match prof_res {
                    Ok(profiles) => {
                        apply_profiles(&ui, profiles);
                        ui.set_status_message(format!("Deleted profile '{name}'").into());
                    }
                    Err(err) => apply_error(&ui, &err),
                }
            });
            return;
        }
        _ => {}
    }

    let is_full_refresh = matches!(
        cmd,
        BackendCommand::Refresh
            | BackendCommand::Create { .. }
            | BackendCommand::DeleteInstance { .. }
    );
    let op_result = execute_instance_command(client, cmd).await;
    let list_result = match op_result {
        Ok(()) => client.list_instances().await,
        Err(err) => Err(err),
    };

    let (images_res, profiles_res, infra_res) = if is_full_refresh && list_result.is_ok() {
        (
            Some(client.list_images().await),
            Some(client.list_profiles().await),
            Some(fetch_infra_snapshot(client).await),
        )
    } else {
        (None, None, None)
    };

    match &list_result {
        Ok(instances) => {
            *last_instances = Some(instances.clone());
            *last_socket_err = false;
        }
        Err(err) => {
            *last_socket_err = err.socket_warning().is_some();
        }
    }

    let weak_done = ui_weak.clone();
    let _ = weak_done.upgrade_in_event_loop(move |ui| {
        ui.set_busy(false);
        match list_result {
            Ok(instances) => {
                apply_instances(&ui, instances);
                let mut warnings = Vec::new();
                match images_res {
                    Some(Ok(images)) => apply_images(&ui, images),
                    Some(Err(err)) => warnings.push(format!("images: {err}")),
                    None => {}
                }
                match profiles_res {
                    Some(Ok(profiles)) => apply_profiles(&ui, profiles),
                    Some(Err(err)) => warnings.push(format!("profiles: {err}")),
                    None => {}
                }
                match infra_res {
                    Some(Ok(infra)) => apply_infra(&ui, infra),
                    Some(Err(err)) => warnings.push(format!("infra: {err}")),
                    None => {}
                }
                if !warnings.is_empty() {
                    ui.set_status_message(warnings.join("; ").into());
                }
            }
            Err(err) => apply_error(&ui, &err),
        }
    });
}

async fn execute_infra_command(
    client: &IncusClient,
    cmd: BackendCommand,
) -> Result<(), IncusError> {
    match cmd {
        BackendCommand::CreateStorageVolume { pool, name } => {
            client.create_storage_volume(&pool, &name).await
        }
        BackendCommand::DeleteStorageVolume { pool, name } => {
            client.delete_storage_volume(&pool, &name).await
        }
        BackendCommand::CreateNetwork { name, nat } => client.create_network(&name, nat).await,
        BackendCommand::DeleteNetwork { name } => client.delete_network(&name).await,
        _ => Ok(()),
    }
}

async fn fetch_infra_snapshot(client: &IncusClient) -> Result<InfraSnapshot, IncusError> {
    let pools = client.list_storage_pools().await?;
    let mut volumes = Vec::new();
    for pool in &pools {
        let pool_vols = client.list_storage_volumes(&pool.name).await?;
        volumes.extend(pool_vols);
    }
    let networks = client.list_networks().await?;
    Ok(InfraSnapshot {
        pools,
        volumes,
        networks,
    })
}

async fn push_file_from_host(
    client: &IncusClient,
    instance: &str,
    host_path: &str,
    container_path: &str,
) -> Result<String, String> {
    let resolved = expand_host_path(host_path)?;
    let bytes = tokio::fs::read(&resolved)
        .await
        .map_err(|e| format!("Failed reading '{}': {e}", resolved.display()))?;
    let len = bytes.len();
    client
        .push_instance_file(instance, container_path, bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!("Pushed {len} bytes to {instance}:{container_path}"))
}

async fn pull_file_to_host(
    client: &IncusClient,
    instance: &str,
    container_path: &str,
    host_path: &str,
) -> Result<String, String> {
    let resolved = expand_host_path(host_path)?;
    let bytes = client
        .pull_instance_file(instance, container_path)
        .await
        .map_err(|e| e.to_string())?;
    let len = bytes.len();
    tokio::fs::write(&resolved, &bytes)
        .await
        .map_err(|e| format!("Failed writing '{}': {e}", resolved.display()))?;
    Ok(format!(
        "Pulled {len} bytes from {instance}:{container_path}"
    ))
}

fn expand_host_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Host path cannot be empty".to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| "Cannot expand '~': HOME is not set".to_string())?;
        return Ok(PathBuf::from(home).join(rest));
    }
    Ok(PathBuf::from(trimmed))
}

async fn load_or_cached_spec(
    slug: &str,
    community_catalog: &Arc<Mutex<Vec<CommunityApp>>>,
    cached_spec: &mut Option<CommunityAppSpec>,
) -> anyhow::Result<CommunityAppSpec> {
    if let Some(existing) = cached_spec
        && existing.slug == slug
    {
        return Ok(existing.clone());
    }
    let (repo, description) = {
        let guard = community_catalog
            .lock()
            .expect("community catalog mutex poisoned");
        guard
            .iter()
            .find(|a| a.slug == slug)
            .map_or((CommunityRepo::Incus, String::new()), |a| {
                (a.repo, a.description.clone())
            })
    };
    let spec = fetch_community_app_spec(slug, repo, &description).await?;
    *cached_spec = Some(spec.clone());
    Ok(spec)
}

struct DeployCommunityRequest<'a> {
    slug: &'a str,
    instance_name: &'a str,
    image_alias: &'a str,
    cpu: &'a str,
    memory: &'a str,
    nesting: NestingMode,
}

async fn deploy_community_sandbox(
    client: &IncusClient,
    req: DeployCommunityRequest<'_>,
    community_catalog: &Arc<Mutex<Vec<CommunityApp>>>,
    cached_spec: &mut Option<CommunityAppSpec>,
) -> anyhow::Result<ExecOutput> {
    let spec = load_or_cached_spec(req.slug, community_catalog, cached_spec).await?;
    client
        .create_instance(CreateInstanceParams {
            name: req.instance_name,
            instance_type: "container",
            image_alias: req.image_alias,
            memory_limit: req.memory,
            cpu_limit: req.cpu,
            workspace_path: "",
            profiles: Vec::new(),
            cloud_init_user_data: "",
            nesting: req.nesting,
        })
        .await?;
    client
        .set_instance_state(req.instance_name, "start")
        .await?;
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if let Ok(list) = client.list_instances().await
            && list
                .iter()
                .any(|i| i.name == req.instance_name && i.ip != "-")
        {
            break;
        }
    }
    execute_community_install(client, &spec, req.instance_name, req.image_alias).await
}

async fn execute_community_install(
    client: &IncusClient,
    spec: &CommunityAppSpec,
    instance_name: &str,
    image_alias: &str,
) -> anyhow::Result<ExecOutput> {
    let bootstrap = vec![
        "sh".to_string(),
        "-c".to_string(),
        "command -v bash >/dev/null 2>&1 || (command -v apk >/dev/null 2>&1 && apk update >/dev/null && apk add --no-cache bash curl >/dev/null)".to_string(),
    ];
    let _ = client.exec_command(instance_name, &bootstrap).await?;

    let install_func = fetch_install_func().await?;
    let runner = build_container_runner_script(spec, instance_name, image_alias);

    client
        .push_instance_file(
            instance_name,
            "/tmp/incus-functions",
            install_func.into_bytes(),
        )
        .await?;
    client
        .push_instance_file(
            instance_name,
            "/tmp/cubby-install.sh",
            spec.install_script.as_bytes().to_vec(),
        )
        .await?;
    client
        .push_instance_file(instance_name, "/tmp/cubby-run.sh", runner.into_bytes())
        .await?;

    let exec_cmd = vec!["bash".to_string(), "/tmp/cubby-run.sh".to_string()];
    Ok(client.exec_command(instance_name, &exec_cmd).await?)
}

async fn execute_instance_command(
    client: &IncusClient,
    cmd: BackendCommand,
) -> Result<(), IncusError> {
    match cmd {
        BackendCommand::Create {
            name,
            instance_type,
            image,
            memory,
            cpu,
            workspace,
            profiles,
            cloud_init,
            nesting,
        } => {
            client
                .create_instance(CreateInstanceParams {
                    name: &name,
                    instance_type: &instance_type,
                    image_alias: &image,
                    memory_limit: &memory,
                    cpu_limit: &cpu,
                    workspace_path: &workspace,
                    profiles,
                    cloud_init_user_data: &cloud_init,
                    nesting,
                })
                .await
        }
        BackendCommand::Clone { source, new_name } => {
            client.clone_instance(&source, &new_name).await
        }
        BackendCommand::SetState { name, action } => {
            client.set_instance_state(&name, &action).await
        }
        BackendCommand::UpdateLimits { name, cpu, memory } => {
            client.update_instance_limits(&name, &cpu, &memory).await
        }
        BackendCommand::DeleteInstance { name } => client.delete_instance(&name).await,
        BackendCommand::AddDiskMount {
            instance,
            dev_name,
            host_path,
            container_path,
        } => {
            client
                .add_disk_mount(&instance, &dev_name, &host_path, &container_path)
                .await
        }
        BackendCommand::AttachCustomVolume {
            instance,
            dev_name,
            pool,
            volume,
            container_path,
        } => {
            client
                .attach_custom_volume(&instance, &dev_name, &pool, &volume, &container_path)
                .await
        }
        BackendCommand::AddProxyDevice {
            instance,
            dev_name,
            host_port,
            container_port,
        } => {
            client
                .add_proxy_device(&instance, &dev_name, host_port, container_port)
                .await
        }
        BackendCommand::RemoveDevice { instance, dev_name } => {
            client.remove_device(&instance, &dev_name).await
        }
        BackendCommand::CreateSnapshot { instance, snapshot } => {
            client.create_snapshot(&instance, &snapshot).await
        }
        BackendCommand::RestoreSnapshot { instance, snapshot } => {
            client.restore_snapshot(&instance, &snapshot).await
        }
        BackendCommand::DeleteSnapshot { instance, snapshot } => {
            client.delete_snapshot(&instance, &snapshot).await
        }
        _ => Ok(()),
    }
}

fn wire_ui_callbacks(
    ui: &AppWindow,
    tx: &mpsc::UnboundedSender<BackendCommand>,
    initial_settings: AppSettings,
    community_catalog: &Arc<Mutex<Vec<CommunityApp>>>,
) {
    let pending_confirm: Rc<RefCell<Option<BackendCommand>>> = Rc::new(RefCell::new(None));
    let settings_state: Rc<RefCell<AppSettings>> = Rc::new(RefCell::new(initial_settings));

    {
        let tx = tx.clone();
        ui.on_refresh(move || {
            let _ = tx.send(BackendCommand::Refresh);
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_select_instance(move |idx| {
            if let Some(ui) = weak.upgrade() {
                clear_confirm(&ui, &pending);
                select_row(&ui, idx);
            }
        });
    }

    {
        let tx = tx.clone();
        ui.on_create_instance(
            move |name,
                  instance_type,
                  image,
                  memory,
                  cpu,
                  workspace,
                  profiles_raw,
                  cloud_init,
                  nesting| {
                let profiles: Vec<String> = profiles_raw
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                let nesting = if nesting {
                    NestingMode::Enabled
                } else {
                    NestingMode::Disabled
                };
                let _ = tx.send(BackendCommand::Create {
                    name: name.trim().to_string(),
                    instance_type: instance_type.trim().to_string(),
                    image: image.trim().to_string(),
                    memory: memory.to_string(),
                    cpu: cpu.to_string(),
                    workspace: workspace.trim().to_string(),
                    profiles,
                    cloud_init: cloud_init.to_string(),
                    nesting,
                });
            },
        );
    }

    {
        let tx = tx.clone();
        ui.on_clone_instance(move |source, new_name| {
            let _ = tx.send(BackendCommand::Clone {
                source: source.to_string(),
                new_name: new_name.trim().to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_set_instance_state(move |name, action| {
            let _ = tx.send(BackendCommand::SetState {
                name: name.to_string(),
                action: action.to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_update_instance_limits(move |name, cpu, memory| {
            let _ = tx.send(BackendCommand::UpdateLimits {
                name: name.to_string(),
                cpu: cpu.trim().to_string(),
                memory: memory.trim().to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_add_disk_mount(move |instance, dev_name, host_path, container_path| {
            let _ = tx.send(BackendCommand::AddDiskMount {
                instance: instance.to_string(),
                dev_name: dev_name.trim().to_string(),
                host_path: host_path.trim().to_string(),
                container_path: container_path.trim().to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_attach_custom_volume(move |instance, dev_name, pool, volume, container_path| {
            let _ = tx.send(BackendCommand::AttachCustomVolume {
                instance: instance.to_string(),
                dev_name: dev_name.trim().to_string(),
                pool: pool.trim().to_string(),
                volume: volume.trim().to_string(),
                container_path: container_path.trim().to_string(),
            });
        });
    }

    {
        let weak = ui.as_weak();
        let tx = tx.clone();
        ui.on_add_proxy_device(move |instance, dev_name, host_port_str, ctr_port_str| {
            let host_port = host_port_str.trim().parse::<u16>();
            let container_port = ctr_port_str.trim().parse::<u16>();
            match (host_port, container_port) {
                (Ok(hp), Ok(cp)) => {
                    let _ = tx.send(BackendCommand::AddProxyDevice {
                        instance: instance.to_string(),
                        dev_name: dev_name.trim().to_string(),
                        host_port: hp,
                        container_port: cp,
                    });
                }
                _ => {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_status_message("Ports must be integers between 1 and 65535".into());
                    }
                }
            }
        });
    }

    {
        let tx = tx.clone();
        ui.on_remove_device(move |instance, dev_name| {
            let _ = tx.send(BackendCommand::RemoveDevice {
                instance: instance.to_string(),
                dev_name: dev_name.to_string(),
            });
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_request_delete_instance(move |name| {
            if let Some(ui) = weak.upgrade() {
                let instance = name.to_string();
                let msg = format!(
                    "Delete sandbox '{instance}' and all its snapshots? This cannot be undone."
                );
                request_confirm(
                    &ui,
                    &pending,
                    &msg,
                    BackendCommand::DeleteInstance { name: instance },
                );
            }
        });
    }

    {
        let tx = tx.clone();
        ui.on_create_snapshot(move |instance, snapshot| {
            let _ = tx.send(BackendCommand::CreateSnapshot {
                instance: instance.to_string(),
                snapshot: snapshot.trim().to_string(),
            });
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_request_restore_snapshot(move |instance, snapshot| {
            if let Some(ui) = weak.upgrade() {
                let inst = instance.to_string();
                let snap = snapshot.to_string();
                let msg = format!(
                    "Restore sandbox '{inst}' to snapshot '{snap}'? Current container changes will be overwritten."
                );
                request_confirm(
                    &ui,
                    &pending,
                    &msg,
                    BackendCommand::RestoreSnapshot {
                        instance: inst,
                        snapshot: snap,
                    },
                );
            }
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_request_delete_snapshot(move |instance, snapshot| {
            if let Some(ui) = weak.upgrade() {
                let inst = instance.to_string();
                let snap = snapshot.to_string();
                let msg = format!("Delete snapshot '{inst}/{snap}'? This cannot be undone.");
                request_confirm(
                    &ui,
                    &pending,
                    &msg,
                    BackendCommand::DeleteSnapshot {
                        instance: inst,
                        snapshot: snap,
                    },
                );
            }
        });
    }

    {
        let weak = ui.as_weak();
        let tx = tx.clone();
        ui.on_upgrade_packages(move |instance, image_os| {
            if let Some(cmd) = upgrade_command(image_os.as_str()) {
                let _ = tx.send(BackendCommand::ExecCommand {
                    instance: instance.to_string(),
                    label: "Upgrading packages".to_string(),
                    command: cmd,
                });
            } else if let Some(ui) = weak.upgrade() {
                ui.set_status_message(
                    format!("Unsupported OS for auto-upgrade: '{image_os}'").into(),
                );
            }
        });
    }

    {
        let weak = ui.as_weak();
        let tx = tx.clone();
        ui.on_install_packages(move |instance, image_os, packages| {
            match install_command(image_os.as_str(), packages.as_str()) {
                Ok(cmd) => {
                    let _ = tx.send(BackendCommand::ExecCommand {
                        instance: instance.to_string(),
                        label: "Installing packages".to_string(),
                        command: cmd,
                    });
                }
                Err(msg) => {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_status_message(msg.into());
                    }
                }
            }
        });
    }

    {
        let tx = tx.clone();
        ui.on_refresh_instance_logs(move |instance| {
            let _ = tx.send(BackendCommand::RefreshInstanceLogs {
                instance: instance.to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_read_instance_log(move |instance, log_name| {
            let _ = tx.send(BackendCommand::ReadInstanceLog {
                instance: instance.to_string(),
                log_name: log_name.to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_push_instance_file(move |instance, host_path, container_path| {
            let _ = tx.send(BackendCommand::PushInstanceFile {
                instance: instance.to_string(),
                host_path: host_path.to_string(),
                container_path: container_path.to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_pull_instance_file(move |instance, container_path, host_path| {
            let _ = tx.send(BackendCommand::PullInstanceFile {
                instance: instance.to_string(),
                container_path: container_path.to_string(),
                host_path: host_path.to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_refresh_infra(move || {
            let _ = tx.send(BackendCommand::RefreshInfra);
        });
    }

    {
        let tx = tx.clone();
        ui.on_create_storage_volume(move |pool, name| {
            let _ = tx.send(BackendCommand::CreateStorageVolume {
                pool: pool.trim().to_string(),
                name: name.trim().to_string(),
            });
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_request_delete_storage_volume(move |pool, name| {
            if let Some(ui) = weak.upgrade() {
                let msg = format!("Delete custom volume '{pool}/{name}'? Data will be lost.");
                request_confirm(
                    &ui,
                    &pending,
                    &msg,
                    BackendCommand::DeleteStorageVolume {
                        pool: pool.to_string(),
                        name: name.to_string(),
                    },
                );
            }
        });
    }

    {
        let tx = tx.clone();
        ui.on_create_network(move |name, nat| {
            let nat = if nat {
                BridgeNat::Enabled
            } else {
                BridgeNat::Disabled
            };
            let _ = tx.send(BackendCommand::CreateNetwork {
                name: name.trim().to_string(),
                nat,
            });
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_request_delete_network(move |name| {
            if let Some(ui) = weak.upgrade() {
                let net = name.to_string();
                let msg = format!("Delete managed network '{net}'?");
                request_confirm(
                    &ui,
                    &pending,
                    &msg,
                    BackendCommand::DeleteNetwork { name: net },
                );
            }
        });
    }

    {
        let tx = tx.clone();
        ui.on_save_profile(move |name, description, cpu, memory, is_create| {
            let mode = if is_create {
                ProfileSaveMode::Create
            } else {
                ProfileSaveMode::Update
            };
            let _ = tx.send(BackendCommand::SaveProfile {
                name: name.trim().to_string(),
                description: description.trim().to_string(),
                cpu: cpu.trim().to_string(),
                memory: memory.trim().to_string(),
                mode,
            });
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_request_delete_profile(move |name| {
            if let Some(ui) = weak.upgrade() {
                let prof = name.to_string();
                let msg = format!("Delete profile '{prof}'?");
                request_confirm(
                    &ui,
                    &pending,
                    &msg,
                    BackendCommand::DeleteProfile { name: prof },
                );
            }
        });
    }

    {
        let tx = tx.clone();
        ui.on_fetch_remote_images(move || {
            let _ = tx.send(BackendCommand::FetchRemoteImages);
        });
    }

    {
        let tx = tx.clone();
        ui.on_refresh_local_images(move || {
            let _ = tx.send(BackendCommand::RefreshLocalImages);
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_request_delete_image(move |fingerprint, label| {
            if let Some(ui) = weak.upgrade() {
                let msg = format!("Delete cached base image '{label}'?");
                request_confirm(
                    &ui,
                    &pending,
                    &msg,
                    BackendCommand::DeleteImage {
                        fingerprint: fingerprint.to_string(),
                    },
                );
            }
        });
    }

    {
        let weak = ui.as_weak();
        let settings = Rc::clone(&settings_state);
        ui.on_save_settings(move |term, ide| {
            let updated = AppSettings {
                preferred_terminal: term.to_string(),
                preferred_ide: ide.to_string(),
            };
            let res = updated.save();
            *settings.borrow_mut() = updated;
            if let Some(ui) = weak.upgrade() {
                match res {
                    Ok(()) => ui.set_status_message("Settings saved".into()),
                    Err(err) => {
                        ui.set_status_message(format!("Failed to save settings: {err}").into());
                    }
                }
            }
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        let tx = tx.clone();
        ui.on_confirm_action(move || {
            if let Some(cmd) = pending.borrow_mut().take() {
                let _ = tx.send(cmd);
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_confirm_message("".into());
            }
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_cancel_confirm(move || {
            if let Some(ui) = weak.upgrade() {
                clear_confirm(&ui, &pending);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let settings = Rc::clone(&settings_state);
        ui.on_open_shell(move |name| {
            let term = settings.borrow().preferred_terminal.clone();
            if let Err(err) = launcher::open_shell(name.as_str(), &term)
                && let Some(ui) = weak.upgrade()
            {
                ui.set_status_message(err.to_string().into());
            }
        });
    }

    {
        let weak = ui.as_weak();
        let settings = Rc::clone(&settings_state);
        ui.on_open_ide(move |workspace_path| {
            let ide = settings.borrow().preferred_ide.clone();
            if let Err(err) = launcher::open_ide(&ide, Some(workspace_path.as_str()))
                && let Some(ui) = weak.upgrade()
            {
                ui.set_status_message(err.to_string().into());
            }
        });
    }

    {
        let tx = tx.clone();
        ui.on_fetch_community_catalog(move || {
            let _ = tx.send(BackendCommand::FetchCommunityCatalog);
        });
    }

    {
        let weak = ui.as_weak();
        let catalog = Arc::clone(community_catalog);
        ui.on_filter_community_apps(move |query| {
            if let Some(ui) = weak.upgrade() {
                let apps = catalog
                    .lock()
                    .expect("community catalog mutex poisoned")
                    .clone();
                apply_community_apps(&ui, &apps, query.as_str());
            }
        });
    }

    {
        let tx = tx.clone();
        ui.on_select_community_app(move |slug| {
            let _ = tx.send(BackendCommand::LoadCommunityAppSpec {
                slug: slug.to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_deploy_community_app(move |slug, name, image, cpu, memory, nesting| {
            let nesting = if nesting {
                NestingMode::Enabled
            } else {
                NestingMode::Disabled
            };
            let _ = tx.send(BackendCommand::DeployCommunityApp {
                slug: slug.to_string(),
                instance_name: name.trim().to_string(),
                image_alias: image.trim().to_string(),
                cpu: cpu.trim().to_string(),
                memory: memory.trim().to_string(),
                nesting,
            });
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_request_run_community_in_instance(move |slug, instance_name| {
            if let Some(ui) = weak.upgrade() {
                let slug = slug.to_string();
                let inst = instance_name.to_string();
                let image_alias = ui.get_community_image_alias().to_string();
                let msg = format!(
                    "Execute community-scripts '{slug}' installer inside running sandbox '{inst}'?"
                );
                request_confirm(
                    &ui,
                    &pending,
                    &msg,
                    BackendCommand::RunCommunityInInstance {
                        slug,
                        instance_name: inst,
                        image_alias,
                    },
                );
            }
        });
    }
}

fn request_confirm(
    ui: &AppWindow,
    pending: &Rc<RefCell<Option<BackendCommand>>>,
    message: &str,
    cmd: BackendCommand,
) {
    *pending.borrow_mut() = Some(cmd);
    ui.set_confirm_message(message.into());
}

fn clear_confirm(ui: &AppWindow, pending: &Rc<RefCell<Option<BackendCommand>>>) {
    *pending.borrow_mut() = None;
    ui.set_confirm_message("".into());
}

fn apply_instances(ui: &AppWindow, instances: Vec<InstanceSummary>) {
    let prev_name = if ui.get_has_selection() {
        Some(ui.get_selected_instance().name.to_string())
    } else {
        None
    };

    let ui_rows: Vec<UiInstance> = instances.into_iter().map(to_ui_instance).collect();
    let count = ui_rows.len();
    let next_idx = prev_name
        .and_then(|prev| ui_rows.iter().position(|row| row.name == prev))
        .map_or(if count > 0 { 0 } else { -1 }, |i| i as i32);

    let running_names: Vec<String> = ui_rows
        .iter()
        .filter(|r| r.is_running)
        .map(|r| r.name.to_string())
        .collect();
    let current_target = ui.get_community_target_instance().to_string();
    let next_target = if running_names.contains(&current_target) {
        current_target
    } else {
        running_names.first().cloned().unwrap_or_default()
    };

    let model = ModelRc::from(Rc::new(VecModel::from(ui_rows)));
    ui.set_instances(model);
    ui.set_running_instance_names(to_shared_string_model(running_names));
    ui.set_community_target_instance(next_target.into());
    ui.set_socket_warning("".into());
    ui.set_status_message(format!("{count} sandbox(es)").into());
    select_row(ui, next_idx);
}

fn apply_community_apps(ui: &AppWindow, apps: &[CommunityApp], query: &str) {
    let needle = query.trim().to_ascii_lowercase();
    let rows: Vec<UiCommunityApp> = apps
        .iter()
        .filter(|a| {
            needle.is_empty()
                || a.slug.to_ascii_lowercase().contains(&needle)
                || a.name.to_ascii_lowercase().contains(&needle)
        })
        .map(|a| UiCommunityApp {
            slug: a.slug.clone().into(),
            name: a.name.clone().into(),
            is_dev: a.is_dev,
        })
        .collect();
    ui.set_community_apps(ModelRc::from(Rc::new(VecModel::from(rows))));
}

fn apply_images(ui: &AppWindow, images: Vec<ImageSummary>) {
    let ui_images: Vec<UiImage> = images
        .into_iter()
        .map(|img| UiImage {
            short_fingerprint: img.fingerprint.chars().take(12).collect::<String>().into(),
            fingerprint: img.fingerprint.into(),
            alias: img.alias.into(),
            description: img.description.into(),
            architecture: img.architecture.into(),
            size_display: img.size_display.into(),
        })
        .collect();
    ui.set_local_images(ModelRc::from(Rc::new(VecModel::from(ui_images))));
}

fn apply_profiles(ui: &AppWindow, profiles: Vec<ProfileSummary>) {
    let mut names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
    if names.is_empty() {
        names.push("default".to_string());
    }
    let ui_profiles: Vec<UiProfile> = profiles
        .into_iter()
        .map(|p| UiProfile {
            name: p.name.into(),
            description: p.description.into(),
        })
        .collect();
    ui.set_profiles(ModelRc::from(Rc::new(VecModel::from(ui_profiles))));
    ui.set_profile_names(to_shared_string_model(names));
}

fn apply_infra(ui: &AppWindow, infra: InfraSnapshot) {
    let mut pool_names: Vec<String> = infra.pools.iter().map(|p| p.name.clone()).collect();
    if pool_names.is_empty() {
        pool_names.push("default".to_string());
    }
    let ui_pools: Vec<UiStoragePool> = infra
        .pools
        .into_iter()
        .map(|p| UiStoragePool {
            name: p.name.into(),
            driver: p.driver.into(),
            status: p.status.into(),
            used_display: p.used_display.into(),
            total_display: p.total_display.into(),
        })
        .collect();
    let ui_volumes: Vec<UiStorageVolume> = infra
        .volumes
        .into_iter()
        .map(|v| UiStorageVolume {
            name: v.name.into(),
            pool: v.pool.into(),
            content_type: v.content_type.into(),
        })
        .collect();
    let ui_networks: Vec<UiNetwork> = infra
        .networks
        .into_iter()
        .map(|n| {
            let leases_display = n
                .leases
                .into_iter()
                .map(|l| format!("{} ({})", l.hostname, l.address))
                .collect::<Vec<_>>()
                .join(", ");
            UiNetwork {
                name: n.name.into(),
                net_type: n.net_type.into(),
                managed: n.managed,
                ipv4: n.ipv4.into(),
                nat: n.nat,
                leases_display: leases_display.into(),
            }
        })
        .collect();

    ui.set_pool_names(to_shared_string_model(pool_names));
    ui.set_storage_pools(ModelRc::from(Rc::new(VecModel::from(ui_pools))));
    ui.set_storage_volumes(ModelRc::from(Rc::new(VecModel::from(ui_volumes))));
    ui.set_networks(ModelRc::from(Rc::new(VecModel::from(ui_networks))));
}

fn format_exec_output(out: &ExecOutput) -> String {
    let mut text = format!("[exit {}]\n", out.exit_code);
    if !out.stdout.is_empty() {
        text.push_str(&out.stdout);
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    if !out.stderr.is_empty() {
        text.push_str("[stderr]\n");
        text.push_str(&out.stderr);
    }
    text
}

fn apply_exec_output(ui: &AppWindow, label: &str, out: &ExecOutput) {
    ui.set_exec_output(format_exec_output(out).into());
    ui.set_status_message(format!("{label} finished (exit {})", out.exit_code).into());
}

fn apply_error(ui: &AppWindow, err: &IncusError) {
    if let Some(warning) = err.socket_warning() {
        ui.set_socket_warning(warning.into());
        ui.set_status_message("Incus socket unavailable".into());
    } else {
        ui.set_status_message(err.to_string().into());
    }
}

fn select_row(ui: &AppWindow, idx: i32) {
    let prev_name = ui
        .get_has_selection()
        .then(|| ui.get_selected_instance().name);
    let model = ui.get_instances();
    if idx >= 0
        && let Some(row) = model.row_data(idx as usize)
    {
        if prev_name.as_ref() != Some(&row.name) {
            clear_instance_views(ui);
        }
        ui.set_selected_index(idx);
        ui.set_selected_instance(row);
        ui.set_has_selection(true);
    } else {
        if prev_name.is_some() {
            clear_instance_views(ui);
        }
        ui.set_selected_index(-1);
        ui.set_has_selection(false);
    }
}

fn clear_instance_views(ui: &AppWindow) {
    ui.set_exec_output("".into());
    ui.set_log_content("".into());
    ui.set_instance_logs(to_shared_string_model(Vec::new()));
    ui.set_selected_log("".into());
}

fn to_shared_string_model(items: Vec<String>) -> ModelRc<SharedString> {
    let shared: Vec<SharedString> = items.into_iter().map(SharedString::from).collect();
    ModelRc::from(Rc::new(VecModel::from(shared)))
}

fn to_ui_instance(inst: InstanceSummary) -> UiInstance {
    let snaps: Vec<UiSnapshot> = inst
        .snapshots
        .into_iter()
        .map(|s| UiSnapshot {
            name: s.name.into(),
            created_at: s.created_at.into(),
        })
        .collect();

    let mounts: Vec<UiMount> = inst
        .mounts
        .into_iter()
        .map(|m| UiMount {
            name: m.name.into(),
            host_path: m.host_path.into(),
            container_path: m.container_path.into(),
        })
        .collect();

    let proxies: Vec<UiProxy> = inst
        .proxies
        .into_iter()
        .map(|p| UiProxy {
            name: p.name.into(),
            listen: p.listen.into(),
            connect: p.connect.into(),
        })
        .collect();

    let profiles_str = if inst.profiles.is_empty() {
        "-".to_string()
    } else {
        inst.profiles.join(", ")
    };

    UiInstance {
        name: inst.name.into(),
        status: inst.status.into(),
        instance_type: inst.instance_type.into(),
        arch: inst.architecture.into(),
        image: inst.image.into(),
        image_os: inst.image_os.into(),
        ip: inst.ip.into(),
        memory: inst.memory.into(),
        cpu: inst.cpu.into(),
        profiles: profiles_str.into(),
        workspace_path: inst.workspace_path.into(),
        cpu_usage_display: inst.cpu_usage_display.into(),
        disk_usage_display: inst.disk_usage_display.into(),
        net_rx_display: inst.net_rx_display.into(),
        net_tx_display: inst.net_tx_display.into(),
        processes_display: inst.processes_display.into(),
        is_running: inst.is_running,
        is_frozen: inst.is_frozen,
        snapshots: ModelRc::from(Rc::new(VecModel::from(snaps))),
        mounts: ModelRc::from(Rc::new(VecModel::from(mounts))),
        proxies: ModelRc::from(Rc::new(VecModel::from(proxies))),
    }
}
