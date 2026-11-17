mod incus;
mod launcher;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::{Model, ModelRc, VecModel, Weak};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;

use crate::incus::{IncusClient, IncusError, InstanceSummary};
use crate::launcher::Ide;

slint::include_modules!();

enum BackendCommand {
    Refresh,
    Create {
        name: String,
        image: String,
        memory: String,
        cpu: String,
        workspace: String,
    },
    SetState {
        name: String,
        action: &'static str,
    },
    DeleteInstance {
        name: String,
    },
    SetWorkspaceMount {
        name: String,
        workspace: String,
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
}

impl BackendCommand {
    fn busy_label(&self) -> String {
        match self {
            Self::Refresh => "Refreshing sandboxes...".to_string(),
            Self::Create { name, image, .. } => format!("Creating {name} ({image})..."),
            Self::SetState { name, action } => format!("Running '{action}' on {name}..."),
            Self::DeleteInstance { name } => format!("Deleting {name}..."),
            Self::SetWorkspaceMount { name, workspace } if workspace.trim().is_empty() => {
                format!("Unmounting workspace on {name}...")
            }
            Self::SetWorkspaceMount { name, .. } => {
                format!("Updating workspace mount on {name}...")
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
        }
    }
}

fn main() -> anyhow::Result<()> {
    let ui = AppWindow::new()?;
    let (tx, rx) = mpsc::unbounded_channel::<BackendCommand>();

    spawn_backend_worker(ui.as_weak(), rx);
    wire_ui_callbacks(&ui, tx.clone());

    let _ = tx.send(BackendCommand::Refresh);
    ui.run()?;
    Ok(())
}

fn spawn_backend_worker(ui_weak: Weak<AppWindow>, mut rx: mpsc::UnboundedReceiver<BackendCommand>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to initialize Tokio runtime");

        rt.block_on(async move {
            let client = IncusClient::default_socket();
            let mut poll_tick = tokio::time::interval(Duration::from_secs(3));
            poll_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
            // Skip immediate first tick since main() sends an explicit Refresh.
            poll_tick.tick().await;

            let mut last_instances: Option<Vec<InstanceSummary>> = None;
            let mut last_socket_err = false;

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

                        let op_result = execute_command(&client, cmd).await;
                        let list_result = match op_result {
                            Ok(()) => client.list_instances().await,
                            Err(err) => Err(err),
                        };

                        match &list_result {
                            Ok(instances) => {
                                last_instances = Some(instances.clone());
                                last_socket_err = false;
                            }
                            Err(err) => {
                                last_socket_err = err.socket_warning().is_some();
                            }
                        }

                        let weak_done = ui_weak.clone();
                        let _ = weak_done.upgrade_in_event_loop(move |ui| {
                            ui.set_busy(false);
                            match list_result {
                                Ok(instances) => apply_instances(&ui, instances),
                                Err(err) => apply_error(&ui, &err),
                            }
                        });
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

async fn execute_command(client: &IncusClient, cmd: BackendCommand) -> Result<(), IncusError> {
    match cmd {
        BackendCommand::Refresh => Ok(()),
        BackendCommand::Create {
            name,
            image,
            memory,
            cpu,
            workspace,
        } => {
            client
                .create_instance(&name, &image, &memory, &cpu, &workspace)
                .await
        }
        BackendCommand::SetState { name, action } => client.set_instance_state(&name, action).await,
        BackendCommand::DeleteInstance { name } => client.delete_instance(&name).await,
        BackendCommand::SetWorkspaceMount { name, workspace } => {
            client.set_workspace_mount(&name, &workspace).await
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
    }
}

fn wire_ui_callbacks(ui: &AppWindow, tx: mpsc::UnboundedSender<BackendCommand>) {
    let pending_confirm: Rc<RefCell<Option<BackendCommand>>> = Rc::new(RefCell::new(None));

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
        ui.on_create_instance(move |name, image, memory, cpu, workspace| {
            let _ = tx.send(BackendCommand::Create {
                name: name.trim().to_string(),
                image: image.trim().to_string(),
                memory: memory.to_string(),
                cpu: cpu.to_string(),
                workspace: workspace.trim().to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_start_instance(move |name| {
            let _ = tx.send(BackendCommand::SetState {
                name: name.to_string(),
                action: "start",
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_stop_instance(move |name| {
            let _ = tx.send(BackendCommand::SetState {
                name: name.to_string(),
                action: "stop",
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_set_workspace_mount(move |name, workspace| {
            let _ = tx.send(BackendCommand::SetWorkspaceMount {
                name: name.to_string(),
                workspace: workspace.trim().to_string(),
            });
        });
    }

    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_confirm);
        ui.on_request_delete_instance(move |name| {
            if let Some(ui) = weak.upgrade() {
                let instance = name.to_string();
                let msg = format!("Delete sandbox '{instance}' and all its snapshots? This cannot be undone.");
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
        ui.on_open_shell(move |name| {
            if let Err(err) = launcher::open_shell(name.as_str())
                && let Some(ui) = weak.upgrade()
            {
                ui.set_status_message(err.to_string().into());
            }
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_open_codium(move |workspace_path| {
            if let Err(err) = launcher::open_ide(Ide::Codium, Some(workspace_path.as_str()))
                && let Some(ui) = weak.upgrade()
            {
                ui.set_status_message(err.to_string().into());
            }
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_open_zed(move |workspace_path| {
            if let Err(err) = launcher::open_ide(Ide::Zed, Some(workspace_path.as_str()))
                && let Some(ui) = weak.upgrade()
            {
                ui.set_status_message(err.to_string().into());
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
        .map(|i| i as i32)
        .unwrap_or(if count > 0 { 0 } else { -1 });

    let model = ModelRc::from(Rc::new(VecModel::from(ui_rows)));
    ui.set_instances(model);
    ui.set_socket_warning("".into());
    ui.set_status_message(format!("{count} sandbox(es)").into());
    select_row(ui, next_idx);
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
    let prev_name = if ui.get_has_selection() {
        Some(ui.get_selected_instance().name)
    } else {
        None
    };
    let model = ui.get_instances();
    if idx >= 0
        && let Some(row) = model.row_data(idx as usize)
    {
        let selection_changed = prev_name.as_ref() != Some(&row.name);
        if selection_changed {
            ui.set_workspace_edit_text(row.workspace_path.clone());
        }
        ui.set_selected_index(idx);
        ui.set_selected_instance(row);
        ui.set_has_selection(true);
    } else {
        ui.set_selected_index(-1);
        ui.set_has_selection(false);
        ui.set_workspace_edit_text("".into());
    }
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

    UiInstance {
        name: inst.name.into(),
        status: inst.status.into(),
        arch: inst.architecture.into(),
        image: inst.image.into(),
        ip: inst.ip.into(),
        memory: inst.memory.into(),
        cpu: inst.cpu.into(),
        workspace_path: inst.workspace_path.into(),
        is_running: inst.is_running,
        snapshots: ModelRc::from(Rc::new(VecModel::from(snaps))),
    }
}
