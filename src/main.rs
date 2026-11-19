mod incus;
mod launcher;
mod logos;
mod settings;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{
    ComponentHandle, Image, LogicalSize, Model, ModelNotify, ModelRc, ModelTracker, SharedString,
    VecModel, Weak,
};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;

use crate::incus::community::{
    CommunityApp, CommunityAppSpec, CommunityRepo, build_container_runner_script,
    curated_community_apps, fetch_community_app_spec, fetch_community_catalog, fetch_install_func,
};
use crate::incus::packages::{install_command, package_manager_label, upgrade_command};
use crate::incus::remote_images::{RemoteImage, default_preset_images, fetch_remote_images};
use crate::incus::{
    BridgeNat, CreateInstanceParams, ExecOutput, ImageSummary, IncusClient, IncusError,
    InstanceSummary, NestingMode, NetworkSummary, ProfileSummary, StoragePoolSummary,
    StorageVolumeSummary, detect_distro_id,
};
use crate::logos::{LogoRequest, spawn_logo_worker};
use crate::settings::{
    AppSettings, available_ides, available_terminals, display_ide_label, display_terminal_label,
};

slint::include_modules!();

enum ProfileSaveMode {
    Create,
    Update,
}

enum PendingConfirm {
    Backend(BackendCommand),
    ResetSettings,
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
        workspace: String,
        profiles: Vec<String>,
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

struct CatalogCaches {
    community_apps: Mutex<Vec<CommunityApp>>,
    remote_images: Mutex<Vec<RemoteImage>>,
    instances: Mutex<Vec<InstanceSummary>>,
    local_images: Mutex<Vec<ImageSummary>>,
}

struct InfraSnapshot {
    pools: Vec<StoragePoolSummary>,
    volumes: Vec<StorageVolumeSummary>,
    networks: Vec<NetworkSummary>,
}

#[derive(Default)]
struct UiLogoStore {
    images: HashMap<String, Image>,
    requested: HashMap<String, String>,
    community_model: Option<Rc<CommunityAppListModel>>,
    logo_tx: Option<mpsc::UnboundedSender<LogoRequest>>,
}

thread_local! {
    static UI_LOGO_STORE: RefCell<UiLogoStore> = RefCell::new(UiLogoStore::default());
}

struct CommunityAppListModel {
    items: RefCell<Vec<CommunityApp>>,
    notify: ModelNotify,
}

impl CommunityAppListModel {
    fn new(items: Vec<CommunityApp>) -> Self {
        Self {
            items: RefCell::new(items),
            notify: ModelNotify::default(),
        }
    }

    fn set_apps(&self, apps: Vec<CommunityApp>) {
        *self.items.borrow_mut() = apps;
        self.notify.reset();
    }

    fn notify_slug_changed(&self, slug: &str) {
        let idx = self.items.borrow().iter().position(|app| app.slug == slug);
        if let Some(row) = idx {
            self.notify.row_changed(row);
        }
    }
}

impl Model for CommunityAppListModel {
    type Data = UiCommunityApp;

    fn row_count(&self) -> usize {
        self.items.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let app = self.items.borrow().get(row).cloned()?;
        let logo = lookup_or_request_logo(&app.slug, &app.logo_url);
        Some(UiCommunityApp {
            slug: app.slug.into(),
            name: app.name.into(),
            description: app.description.into(),
            distro_id: "generic".into(),
            logo,
            is_dev: app.is_dev,
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn lookup_or_request_logo(slug: &str, url: &str) -> Image {
    if slug.is_empty() {
        return Image::default();
    }
    UI_LOGO_STORE.with(|store| {
        let mut store = store.borrow_mut();
        if let Some(img) = store.images.get(slug) {
            return img.clone();
        }
        let should_request = store.requested.get(slug).is_none_or(|prev| prev != url);
        if should_request {
            store.requested.insert(slug.to_string(), url.to_string());
            if let Some(tx) = store.logo_tx.as_ref() {
                let _ = tx.send(LogoRequest {
                    slug: slug.to_string(),
                    url: url.to_string(),
                });
            }
        }
        Image::default()
    })
}

fn on_logo_ready_ui(ui: &AppWindow, slug: &str, img: Image) {
    let community_model = UI_LOGO_STORE.with(|store| {
        let mut store = store.borrow_mut();
        store.images.insert(slug.to_string(), img.clone());
        store.community_model.clone()
    });

    if let Some(model) = community_model {
        model.notify_slug_changed(slug);
    }

    if ui.get_community_selected_slug().as_str() == slug {
        ui.set_community_app_logo(img.clone());
    }

    let instances_model = ui.get_instances();
    for row in 0..instances_model.row_count() {
        if let Some(mut inst) = instances_model.row_data(row)
            && inst.app_slug.as_str() == slug
        {
            inst.logo = img.clone();
            instances_model.set_row_data(row, inst);
        }
    }

    if ui.get_has_selection() {
        let mut selected = ui.get_selected_instance();
        if selected.app_slug.as_str() == slug {
            selected.logo = img;
            ui.set_selected_instance(selected);
        }
    }
}

fn sync_instance_logos(ui: &AppWindow, apps: &[CommunityApp]) {
    let url_by_slug: HashMap<&str, &str> = apps
        .iter()
        .filter(|a| !a.logo_url.is_empty())
        .map(|a| (a.slug.as_str(), a.logo_url.as_str()))
        .collect();

    let instances_model = ui.get_instances();
    for row in 0..instances_model.row_count() {
        if let Some(mut inst) = instances_model.row_data(row)
            && !inst.app_slug.is_empty()
        {
            let url = url_by_slug
                .get(inst.app_slug.as_str())
                .copied()
                .unwrap_or("");
            let img = lookup_or_request_logo(inst.app_slug.as_str(), url);
            if img.size().width > 0 && inst.logo != img {
                inst.logo = img;
                instances_model.set_row_data(row, inst);
            }
        }
    }

    if ui.get_has_selection() {
        let mut selected = ui.get_selected_instance();
        if !selected.app_slug.is_empty() {
            let url = url_by_slug
                .get(selected.app_slug.as_str())
                .copied()
                .unwrap_or("");
            let img = lookup_or_request_logo(selected.app_slug.as_str(), url);
            if img.size().width > 0 && selected.logo != img {
                selected.logo = img;
                ui.set_selected_instance(selected);
            }
        }
    }

    let selected_slug = ui.get_community_selected_slug().to_string();
    if !selected_slug.is_empty() {
        let url = url_by_slug
            .get(selected_slug.as_str())
            .copied()
            .unwrap_or("");
        let img = lookup_or_request_logo(&selected_slug, url);
        if img.size().width > 0 {
            ui.set_community_app_logo(img);
        }
    }
}

fn community_logo_url(community_apps: &Mutex<Vec<CommunityApp>>, slug: &str) -> String {
    community_apps
        .lock()
        .expect("community catalog mutex poisoned")
        .iter()
        .find(|app| app.slug == slug)
        .map_or_else(String::new, |app| app.logo_url.clone())
}

fn main() -> anyhow::Result<()> {
    let ui = AppWindow::new()?;
    let app_settings = AppSettings::load();
    let catalogs = Arc::new(CatalogCaches {
        community_apps: Mutex::new(curated_community_apps()),
        remote_images: Mutex::new(default_preset_images()),
        instances: Mutex::new(Vec::new()),
        local_images: Mutex::new(Vec::new()),
    });

    let (logo_tx, logo_rx) = mpsc::unbounded_channel::<LogoRequest>();
    let community_model = Rc::new(CommunityAppListModel::new(Vec::new()));
    UI_LOGO_STORE.with(|store| {
        let mut store = store.borrow_mut();
        store.community_model = Some(Rc::clone(&community_model));
        store.logo_tx = Some(logo_tx);
    });
    ui.set_community_apps(ModelRc::from(community_model));

    init_settings_ui(&ui, &app_settings, &catalogs);

    let (tx, rx) = mpsc::unbounded_channel::<BackendCommand>();
    spawn_backend_worker(ui.as_weak(), rx, logo_rx, Arc::clone(&catalogs));
    wire_ui_callbacks(&ui, &tx, app_settings, &catalogs);

    let _ = tx.send(BackendCommand::Refresh);
    ui.run()?;
    Ok(())
}

fn init_settings_ui(ui: &AppWindow, app_settings: &AppSettings, catalogs: &Arc<CatalogCaches>) {
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
    ui.set_preferred_terminal(term_pref.clone().into());
    ui.set_preferred_ide(ide_pref.clone().into());
    ui.set_resolved_terminal_name(display_terminal_label(&term_pref).into());
    ui.set_resolved_ide_name(display_ide_label(&ide_pref).into());
    ui.set_settings_file_path(AppSettings::display_config_path().into());
    ui.set_simple_mode(app_settings.simple_mode);
    ui.set_light_theme(app_settings.light_theme);
    if app_settings.simple_mode {
        ui.window().set_size(LogicalSize::new(520.0, 580.0));
    }

    let remote_imgs = catalogs
        .remote_images
        .lock()
        .expect("remote images mutex poisoned")
        .clone();
    let local_imgs = catalogs
        .local_images
        .lock()
        .expect("local images mutex poisoned")
        .clone();
    apply_remote_aliases(ui, &remote_imgs, &local_imgs, "");

    let apps = catalogs
        .community_apps
        .lock()
        .expect("community catalog mutex poisoned")
        .clone();
    apply_community_apps(&apps, "", "all");
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
    logo_rx: mpsc::UnboundedReceiver<LogoRequest>,
    catalogs: Arc<CatalogCaches>,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to initialize Tokio runtime");

        rt.block_on(async move {
            let logo_weak = ui_weak.clone();
            spawn_logo_worker(logo_rx, move |slug, decoded| {
                let weak = logo_weak.clone();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    if let Some(img) = decoded.into_slint_image() {
                        on_logo_ready_ui(&ui, &slug, img);
                    }
                });
            });

            let client = IncusClient::default_socket();
            let mut poll_tick = tokio::time::interval(Duration::from_secs(3));
            poll_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
            poll_tick.tick().await;

            let mut last_instances: Option<Vec<InstanceSummary>> = None;
            let mut last_socket_err = false;
            let mut cached_spec: Option<CommunityAppSpec> = None;

            {
                let weak_startup = ui_weak.clone();
                let catalog_startup = Arc::clone(&catalogs);
                tokio::spawn(async move {
                    if let Ok(apps) = fetch_community_catalog().await {
                        catalog_startup
                            .community_apps
                            .lock()
                            .expect("community catalog mutex poisoned")
                            .clone_from(&apps);
                        let _ = weak_startup.upgrade_in_event_loop(move |ui| {
                            let query = ui.get_community_search_query().to_string();
                            let repo_filter = ui.get_community_repo_filter().to_string();
                            apply_community_apps(&apps, &query, &repo_filter);
                            sync_instance_logos(&ui, &apps);
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
                            ui.set_status_is_idle(false);
                            ui.set_status_message(busy_msg.into());
                        });

                        handle_command(
                            &client,
                            cmd,
                            &ui_weak,
                            &mut last_instances,
                            &mut last_socket_err,
                            &catalogs,
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
                                    catalogs
                                        .instances
                                        .lock()
                                        .expect("instances mutex poisoned")
                                        .clone_from(&instances);
                                    let weak_poll = ui_weak.clone();
                                    let catalog_poll = Arc::clone(&catalogs);
                                    let _ = weak_poll.upgrade_in_event_loop(move |ui| {
                                        if !ui.get_busy() {
                                            apply_instances(
                                                &ui,
                                                &instances,
                                                &catalog_poll.community_apps,
                                            );
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
    catalogs: &Arc<CatalogCaches>,
    cached_spec: &mut Option<CommunityAppSpec>,
) {
    match cmd {
        BackendCommand::FetchCommunityCatalog => {
            let res = fetch_community_catalog().await;
            if let Ok(ref apps) = res {
                catalogs
                    .community_apps
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
                        let repo_filter = ui.get_community_repo_filter().to_string();
                        UI_LOGO_STORE.with(|store| store.borrow_mut().requested.clear());
                        apply_community_apps(&apps, &query, &repo_filter);
                        sync_instance_logos(&ui, &apps);
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
            let res = load_or_cached_spec(&slug, &catalogs.community_apps, cached_spec).await;
            let logo_url = community_logo_url(&catalogs.community_apps, &slug);
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(spec) => {
                        let is_dev = spec.repo == CommunityRepo::DevScripts;
                        let source_repo = match spec.repo {
                            CommunityRepo::Incus => "community-scripts/Incus",
                            CommunityRepo::DevScripts => "community-scripts/DevScripts",
                            CommunityRepo::ProxmoxVe => "community-scripts/ProxmoxVE",
                        };
                        let logo = lookup_or_request_logo(&spec.slug, &logo_url);
                        ui.set_community_selected_slug(spec.slug.into());
                        ui.set_community_app_logo(logo);
                        ui.set_community_app_name(spec.app_name.into());
                        ui.set_community_description(spec.description.into());
                        ui.set_community_tags(spec.tags.into());
                        ui.set_community_is_dev(is_dev);
                        ui.set_community_source_repo(source_repo.into());
                        ui.set_create_image_alias(spec.image_alias.into());
                        ui.set_create_cpu(spec.cpu_limit.into());
                        ui.set_create_memory(spec.memory_limit.into());
                        ui.set_create_nesting(spec.nesting);
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
            workspace,
            profiles,
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
                    workspace: &workspace,
                    profiles,
                    nesting,
                },
                &catalogs.community_apps,
                cached_spec,
            )
            .await;
            let list_res = client.list_instances().await;
            if let Ok(ref instances) = list_res {
                *last_instances = Some(instances.clone());
                *last_socket_err = false;
                catalogs
                    .instances
                    .lock()
                    .expect("instances mutex poisoned")
                    .clone_from(instances);
            }
            let weak = ui_weak.clone();
            let catalog_done = Arc::clone(catalogs);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                if let Ok(instances) = list_res {
                    apply_instances(&ui, &instances, &catalog_done.community_apps);
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
                let spec =
                    load_or_cached_spec(&slug, &catalogs.community_apps, cached_spec).await?;
                let _ = client.tag_cubby_app(&instance_name, &slug).await;
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
            let local_imgs = catalogs
                .local_images
                .lock()
                .expect("local images mutex poisoned")
                .clone();
            if let Ok(ref imgs) = res
                && !imgs.is_empty()
            {
                catalogs
                    .remote_images
                    .lock()
                    .expect("remote images mutex poisoned")
                    .clone_from(imgs);
            }
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(imgs) if !imgs.is_empty() => {
                        let count = imgs.len();
                        let query = ui.get_remote_search_query().to_string();
                        apply_remote_aliases(&ui, &imgs, &local_imgs, &query);
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
            let remote_imgs = catalogs
                .remote_images
                .lock()
                .expect("remote images mutex poisoned")
                .clone();
            if let Ok(ref images) = res {
                catalogs
                    .local_images
                    .lock()
                    .expect("local images mutex poisoned")
                    .clone_from(images);
            }
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match res {
                    Ok(images) => {
                        let img_query = ui.get_image_search_query().to_string();
                        let remote_query = ui.get_remote_search_query().to_string();
                        apply_images(&ui, &images, &img_query);
                        apply_remote_aliases(&ui, &remote_imgs, &images, &remote_query);
                    }
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
            let remote_imgs = catalogs
                .remote_images
                .lock()
                .expect("remote images mutex poisoned")
                .clone();
            if let Ok(ref images) = imgs {
                catalogs
                    .local_images
                    .lock()
                    .expect("local images mutex poisoned")
                    .clone_from(images);
            }
            let weak = ui_weak.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                match imgs {
                    Ok(images) => {
                        let img_query = ui.get_image_search_query().to_string();
                        let remote_query = ui.get_remote_search_query().to_string();
                        apply_images(&ui, &images, &img_query);
                        apply_remote_aliases(&ui, &remote_imgs, &images, &remote_query);
                    }
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
            catalogs
                .instances
                .lock()
                .expect("instances mutex poisoned")
                .clone_from(instances);
        }
        Err(err) => {
            *last_socket_err = err.socket_warning().is_some();
        }
    }
    if let Some(Ok(ref images)) = images_res {
        catalogs
            .local_images
            .lock()
            .expect("local images mutex poisoned")
            .clone_from(images);
    }
    let remote_imgs = catalogs
        .remote_images
        .lock()
        .expect("remote images mutex poisoned")
        .clone();

    let weak_done = ui_weak.clone();
    let catalog_done = Arc::clone(catalogs);
    let _ = weak_done.upgrade_in_event_loop(move |ui| {
        ui.set_busy(false);
        match list_result {
            Ok(instances) => {
                apply_instances(&ui, &instances, &catalog_done.community_apps);
                let mut warnings = Vec::new();
                match images_res {
                    Some(Ok(images)) => {
                        let img_query = ui.get_image_search_query().to_string();
                        let remote_query = ui.get_remote_search_query().to_string();
                        apply_images(&ui, &images, &img_query);
                        apply_remote_aliases(&ui, &remote_imgs, &images, &remote_query);
                    }
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
    community_catalog: &Mutex<Vec<CommunityApp>>,
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
    workspace: &'a str,
    profiles: Vec<String>,
    nesting: NestingMode,
}

async fn deploy_community_sandbox(
    client: &IncusClient,
    req: DeployCommunityRequest<'_>,
    community_catalog: &Mutex<Vec<CommunityApp>>,
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
            workspace_path: req.workspace,
            profiles: req.profiles,
            cloud_init_user_data: "",
            nesting: req.nesting,
        })
        .await?;
    let _ = client.tag_cubby_app(req.instance_name, req.slug).await;
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

fn parse_profiles_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn wire_ui_callbacks(
    ui: &AppWindow,
    tx: &mpsc::UnboundedSender<BackendCommand>,
    initial_settings: AppSettings,
    catalogs: &Arc<CatalogCaches>,
) {
    let pending_confirm: Rc<RefCell<Option<PendingConfirm>>> = Rc::new(RefCell::new(None));
    let settings_state: Rc<RefCell<AppSettings>> = Rc::new(RefCell::new(initial_settings));

    {
        let tx = tx.clone();
        ui.on_refresh(move || {
            let _ = tx.send(BackendCommand::Refresh);
        });
    }

    {
        let weak = ui.as_weak();
        let settings = Rc::clone(&settings_state);
        ui.on_toggle_simple_mode(move |simple| {
            let mut updated = settings.borrow().clone();
            updated.simple_mode = simple;
            let _ = updated.save();
            *settings.borrow_mut() = updated;
            if let Some(ui) = weak.upgrade() {
                ui.set_simple_mode(simple);
                if simple {
                    ui.set_simple_view("hub".into());
                    ui.window().set_size(LogicalSize::new(520.0, 580.0));
                } else {
                    ui.window().set_size(LogicalSize::new(1120.0, 760.0));
                }
            }
        });
    }

    {
        let weak = ui.as_weak();
        let settings = Rc::clone(&settings_state);
        ui.on_toggle_theme(move |light| {
            let mut updated = settings.borrow().clone();
            updated.light_theme = light;
            let _ = updated.save();
            *settings.borrow_mut() = updated;
            if let Some(ui) = weak.upgrade() {
                ui.set_light_theme(light);
            }
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
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_select_instance_by_name(move |name| {
            if let Some(ui) = weak.upgrade() {
                clear_confirm(&ui, &pending);
                let model = ui.get_instances();
                if let Some(idx) = (0..model.row_count())
                    .position(|i| model.row_data(i).is_some_and(|r| r.name == name))
                {
                    select_row(&ui, idx as i32);
                }
            }
        });
    }

    {
        let weak = ui.as_weak();
        let caches = Arc::clone(catalogs);
        ui.on_filter_sandboxes(move |query| {
            if let Some(ui) = weak.upgrade() {
                let all = caches
                    .instances
                    .lock()
                    .expect("instances mutex poisoned")
                    .clone();
                ui.set_sandbox_search_query(query);
                apply_instances(&ui, &all, &caches.community_apps);
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
                let profiles = parse_profiles_csv(&profiles_raw);
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
        let weak = ui.as_weak();
        let caches = Arc::clone(catalogs);
        ui.on_filter_remote_aliases(move |query| {
            if let Some(ui) = weak.upgrade() {
                let remote_imgs = caches
                    .remote_images
                    .lock()
                    .expect("remote images mutex poisoned")
                    .clone();
                let local_imgs = caches
                    .local_images
                    .lock()
                    .expect("local images mutex poisoned")
                    .clone();
                apply_remote_aliases(&ui, &remote_imgs, &local_imgs, query.as_str());
            }
        });
    }

    {
        let weak = ui.as_weak();
        let caches = Arc::clone(catalogs);
        ui.on_filter_local_images(move |query| {
            if let Some(ui) = weak.upgrade() {
                let local_imgs = caches
                    .local_images
                    .lock()
                    .expect("local images mutex poisoned")
                    .clone();
                apply_images(&ui, &local_imgs, query.as_str());
            }
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
            let mut updated = settings.borrow().clone();
            updated.preferred_terminal = term.to_string();
            updated.preferred_ide = ide.to_string();
            let res = updated.save();
            *settings.borrow_mut() = updated.clone();
            if let Some(ui) = weak.upgrade() {
                ui.set_resolved_terminal_name(
                    display_terminal_label(&updated.preferred_terminal).into(),
                );
                ui.set_resolved_ide_name(display_ide_label(&updated.preferred_ide).into());
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
        ui.on_request_reset_settings(move || {
            if let Some(ui) = weak.upgrade() {
                *pending.borrow_mut() = Some(PendingConfirm::ResetSettings);
                ui.set_confirm_message(
                    "Delete local settings.json and reset preferences to defaults?".into(),
                );
            }
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        let settings = Rc::clone(&settings_state);
        let tx = tx.clone();
        ui.on_confirm_action(move || {
            match pending.borrow_mut().take() {
                Some(PendingConfirm::Backend(cmd)) => {
                    let _ = tx.send(cmd);
                }
                Some(PendingConfirm::ResetSettings) => {
                    let res = AppSettings::reset_file();
                    let defaults = AppSettings::default();
                    *settings.borrow_mut() = defaults.clone();
                    if let Some(ui) = weak.upgrade() {
                        ui.set_preferred_terminal("auto".into());
                        ui.set_preferred_ide("auto".into());
                        ui.set_resolved_terminal_name(display_terminal_label("auto").into());
                        ui.set_resolved_ide_name(display_ide_label("auto").into());
                        ui.set_light_theme(false);
                        match res {
                            Ok(()) => ui.set_status_message("Settings file removed".into()),
                            Err(err) => {
                                ui.set_status_message(format!("Reset failed: {err}").into());
                            }
                        }
                    }
                }
                None => {}
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
        ui.on_browse_workspace_folder(move || {
            let weak = weak.clone();
            std::thread::spawn(move || {
                let picked = pick_host_directory();
                let _ = weak.upgrade_in_event_loop(move |ui| match picked {
                    Ok(Some(path)) => ui.set_create_workspace(path.into()),
                    Ok(None) => {}
                    Err(msg) => ui.set_status_message(msg.into()),
                });
            });
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_open_url(move |url| {
            let url_str = url.as_str();
            if !url_str.starts_with("https://github.com/nandazahran/IncusCubby") {
                return;
            }
            if let Err(err) = Command::new("xdg-open").arg(url_str).spawn()
                && let Some(ui) = weak.upgrade()
            {
                ui.set_status_message(format!("Failed opening URL: {err}").into());
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
        let caches = Arc::clone(catalogs);
        ui.on_filter_community_apps(move |query| {
            if let Some(ui) = weak.upgrade() {
                let apps = caches
                    .community_apps
                    .lock()
                    .expect("community catalog mutex poisoned")
                    .clone();
                let repo_filter = ui.get_community_repo_filter().to_string();
                apply_community_apps(&apps, query.as_str(), &repo_filter);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let tx = tx.clone();
        let caches = Arc::clone(catalogs);
        ui.on_select_community_app(move |slug| {
            if let Some(ui) = weak.upgrade() {
                let logo_url = community_logo_url(&caches.community_apps, slug.as_str());
                ui.set_community_app_logo(lookup_or_request_logo(slug.as_str(), &logo_url));
            }
            let _ = tx.send(BackendCommand::LoadCommunityAppSpec {
                slug: slug.to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_deploy_community_app(
            move |slug, name, image, cpu, memory, workspace, profiles_raw, nesting| {
                let profiles = parse_profiles_csv(&profiles_raw);
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
                    workspace: workspace.trim().to_string(),
                    profiles,
                    nesting,
                });
            },
        );
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_request_run_community_in_instance(move |slug, instance_name| {
            if let Some(ui) = weak.upgrade() {
                let slug = slug.to_string();
                let inst = instance_name.to_string();
                let image_alias = ui.get_create_image_alias().to_string();
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

fn pick_host_directory() -> Result<Option<String>, String> {
    if let Ok(out) = Command::new("zenity")
        .args([
            "--file-selection",
            "--directory",
            "--title=Select Workspace Folder",
        ])
        .output()
    {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok((!path.is_empty()).then_some(path));
        }
        return Ok(None);
    }
    if let Ok(out) = Command::new("kdialog")
        .args(["--getexistingdirectory", "."])
        .output()
    {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok((!path.is_empty()).then_some(path));
        }
        return Ok(None);
    }
    Err("Enter workspace folder path directly (zenity/kdialog not found)".to_string())
}

fn request_confirm(
    ui: &AppWindow,
    pending: &Rc<RefCell<Option<PendingConfirm>>>,
    message: &str,
    cmd: BackendCommand,
) {
    *pending.borrow_mut() = Some(PendingConfirm::Backend(cmd));
    ui.set_confirm_message(message.into());
}

fn clear_confirm(ui: &AppWindow, pending: &Rc<RefCell<Option<PendingConfirm>>>) {
    *pending.borrow_mut() = None;
    ui.set_confirm_message("".into());
}

fn collect_instance_logo_urls(
    instances: &[InstanceSummary],
    community_apps: &Mutex<Vec<CommunityApp>>,
) -> HashMap<String, String> {
    let needed: HashSet<&str> = instances
        .iter()
        .filter(|inst| !inst.app_slug.is_empty())
        .map(|inst| inst.app_slug.as_str())
        .collect();
    if needed.is_empty() {
        return HashMap::new();
    }
    let guard = community_apps
        .lock()
        .expect("community catalog mutex poisoned");
    guard
        .iter()
        .filter(|app| !app.logo_url.is_empty() && needed.contains(app.slug.as_str()))
        .map(|app| (app.slug.clone(), app.logo_url.clone()))
        .collect()
}

fn apply_instances(
    ui: &AppWindow,
    instances: &[InstanceSummary],
    community_apps: &Mutex<Vec<CommunityApp>>,
) {
    let prev_name = if ui.get_has_selection() {
        Some(ui.get_selected_instance().name.to_string())
    } else {
        None
    };

    let total_count = instances.len();
    let running_count = instances.iter().filter(|i| i.is_running).count();
    let frozen_count = instances.iter().filter(|i| i.is_frozen).count();
    let running_summary = if frozen_count > 0 {
        format!("{running_count} running · {frozen_count} frozen")
    } else {
        format!("{running_count} running")
    };

    let running_names: Vec<String> = instances
        .iter()
        .filter(|r| r.is_running)
        .map(|r| r.name.clone())
        .collect();
    let current_target = ui.get_community_target_instance().to_string();
    let next_target = if running_names.contains(&current_target) {
        current_target
    } else {
        running_names.first().cloned().unwrap_or_default()
    };

    let app_logo_urls = collect_instance_logo_urls(instances, community_apps);
    let needle = ui.get_sandbox_search_query().trim().to_ascii_lowercase();
    let ui_rows: Vec<UiInstance> = instances
        .iter()
        .filter(|inst| {
            needle.is_empty()
                || inst.name.to_ascii_lowercase().contains(&needle)
                || inst.image.to_ascii_lowercase().contains(&needle)
                || inst.app_slug.to_ascii_lowercase().contains(&needle)
                || inst.ip.to_ascii_lowercase().contains(&needle)
        })
        .cloned()
        .map(|inst| {
            let url = app_logo_urls.get(&inst.app_slug).map_or("", String::as_str);
            to_ui_instance(inst, url)
        })
        .collect();
    let visible_count = ui_rows.len();
    let next_idx = prev_name
        .and_then(|prev| ui_rows.iter().position(|row| row.name == prev))
        .map_or(if visible_count > 0 { 0 } else { -1 }, |i| i as i32);

    let model = ModelRc::from(Rc::new(VecModel::from(ui_rows)));
    ui.set_instances(model);
    ui.set_total_instance_count(total_count as i32);
    ui.set_running_count_summary(running_summary.into());
    ui.set_running_instance_names(to_shared_string_model(running_names));
    ui.set_community_target_instance(next_target.into());
    ui.set_socket_connected(true);
    ui.set_socket_warning("".into());
    ui.set_status_is_idle(true);
    ui.set_status_message(format!("{total_count} sandbox(es)").into());
    select_row(ui, next_idx);
}

fn apply_remote_aliases(
    ui: &AppWindow,
    remote_imgs: &[RemoteImage],
    local_imgs: &[ImageSummary],
    query: &str,
) {
    let cached_aliases: HashSet<String> = local_imgs
        .iter()
        .filter(|img| !img.alias.is_empty())
        .map(|img| img.alias.to_ascii_lowercase())
        .collect();
    let needle = query.trim().to_ascii_lowercase();
    let rows: Vec<UiRemoteAlias> = remote_imgs
        .iter()
        .filter(|img| {
            needle.is_empty()
                || img.alias.to_ascii_lowercase().contains(&needle)
                || img.os.to_ascii_lowercase().contains(&needle)
                || img.release.to_ascii_lowercase().contains(&needle)
        })
        .map(|img| {
            let subtitle = if img.release.is_empty() {
                img.os.clone()
            } else if img.os.is_empty() {
                img.release.clone()
            } else {
                format!("{} · {}", img.os, img.release)
            };
            UiRemoteAlias {
                distro_id: detect_distro_id(&img.os, &img.alias).into(),
                is_cached: cached_aliases.contains(&img.alias.to_ascii_lowercase()),
                alias: img.alias.clone().into(),
                subtitle: subtitle.into(),
                release_hint: img.release.clone().into(),
            }
        })
        .collect();
    ui.set_remote_aliases(ModelRc::from(Rc::new(VecModel::from(rows))));
}

fn apply_community_apps(apps: &[CommunityApp], query: &str, repo_filter: &str) {
    let needle = query.trim().to_ascii_lowercase();
    let filtered: Vec<CommunityApp> = apps
        .iter()
        .filter(|a| match repo_filter {
            "incus" => !a.is_dev,
            "devscripts" => a.is_dev,
            _ => true,
        })
        .filter(|a| {
            needle.is_empty()
                || a.slug.to_ascii_lowercase().contains(&needle)
                || a.name.to_ascii_lowercase().contains(&needle)
                || a.description.to_ascii_lowercase().contains(&needle)
        })
        .cloned()
        .collect();
    let model = UI_LOGO_STORE.with(|store| store.borrow().community_model.clone());
    if let Some(model) = model {
        model.set_apps(filtered);
    }
}

fn apply_images(ui: &AppWindow, images: &[ImageSummary], query: &str) {
    let needle = query.trim().to_ascii_lowercase();
    let ui_images: Vec<UiImage> = images
        .iter()
        .filter(|img| {
            needle.is_empty()
                || img.alias.to_ascii_lowercase().contains(&needle)
                || img.description.to_ascii_lowercase().contains(&needle)
                || img.fingerprint.to_ascii_lowercase().contains(&needle)
        })
        .map(|img| UiImage {
            short_fingerprint: img.fingerprint.chars().take(12).collect::<String>().into(),
            distro_id: detect_distro_id(&img.description, &img.alias).into(),
            fingerprint: img.fingerprint.clone().into(),
            alias: img.alias.clone().into(),
            description: img.description.clone().into(),
            architecture: img.architecture.clone().into(),
            size_display: img.size_display.clone().into(),
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
            cpu: p.cpu.into(),
            memory: p.memory.into(),
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
            used_ratio: f32::from(p.used_per_mille) / 1000.0,
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
                .iter()
                .map(|l| format!("{} ({})", l.hostname, l.address))
                .collect::<Vec<_>>()
                .join(", ");
            let leases: Vec<UiNetworkLease> = n
                .leases
                .into_iter()
                .map(|l| UiNetworkLease {
                    hostname: l.hostname.into(),
                    address: l.address.into(),
                })
                .collect();
            UiNetwork {
                name: n.name.into(),
                net_type: n.net_type.into(),
                managed: n.managed,
                ipv4: n.ipv4.into(),
                nat: n.nat,
                leases_display: leases_display.into(),
                leases: ModelRc::from(Rc::new(VecModel::from(leases))),
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
    ui.set_status_is_idle(false);
    if let Some(warning) = err.socket_warning() {
        ui.set_socket_connected(false);
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
            ui.set_edit_limit_cpu(row.cpu_limit_raw.clone());
            ui.set_edit_limit_mem(row.memory_limit_raw.clone());
        }
        ui.set_selected_index(idx);
        ui.set_selected_instance(row);
        ui.set_has_selection(true);
    } else {
        if prev_name.is_some() {
            clear_instance_views(ui);
            ui.set_edit_limit_cpu("".into());
            ui.set_edit_limit_mem("".into());
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

fn to_ui_instance(inst: InstanceSummary, logo_url: &str) -> UiInstance {
    let logo = if inst.app_slug.is_empty() {
        Image::default()
    } else {
        lookup_or_request_logo(&inst.app_slug, logo_url)
    };
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

    let pkg_manager = package_manager_label(&inst.image_os);
    UiInstance {
        name: inst.name.into(),
        status: inst.status.into(),
        instance_type: inst.instance_type.into(),
        arch: inst.architecture.into(),
        image: inst.image.into(),
        image_os: inst.image_os.into(),
        app_slug: inst.app_slug.into(),
        distro_id: inst.distro_id.into(),
        logo,
        ip: inst.ip.into(),
        net_iface: inst.net_iface.into(),
        memory: inst.memory.into(),
        memory_usage_display: inst.memory_usage_display.into(),
        memory_limit_display: inst.memory_limit_display.into(),
        memory_limit_raw: inst.memory_limit_raw.into(),
        cpu: inst.cpu.into(),
        cpu_limit_raw: inst.cpu_limit_raw.into(),
        root_pool: inst.root_pool.into(),
        profiles: profiles_str.into(),
        workspace_path: inst.workspace_path.into(),
        cpu_usage_display: inst.cpu_usage_display.into(),
        disk_usage_display: inst.disk_usage_display.into(),
        net_rx_display: inst.net_rx_display.into(),
        net_tx_display: inst.net_tx_display.into(),
        processes_display: inst.processes_display.into(),
        pkg_manager: pkg_manager.into(),
        is_running: inst.is_running,
        is_frozen: inst.is_frozen,
        snapshots: ModelRc::from(Rc::new(VecModel::from(snaps))),
        mounts: ModelRc::from(Rc::new(VecModel::from(mounts))),
        proxies: ModelRc::from(Rc::new(VecModel::from(proxies))),
    }
}
