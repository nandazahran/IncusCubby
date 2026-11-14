mod incus;
mod launcher;

use std::rc::Rc;

use slint::{Model, ModelRc, VecModel, Weak};
use tokio::sync::mpsc;

use crate::incus::{IncusClient, IncusError, InstanceSummary};
use crate::launcher::Ide;

slint::include_modules!();

enum BackendCommand {
    Refresh,
    Create {
        name: String,
        image: String,
        memory: String,
    },
    SetState {
        name: String,
        action: &'static str,
    },
    DeleteInstance {
        name: String,
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
            while let Some(cmd) = rx.recv().await {
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

                let weak_done = ui_weak.clone();
                let _ = weak_done.upgrade_in_event_loop(move |ui| {
                    ui.set_busy(false);
                    match list_result {
                        Ok(instances) => apply_instances(&ui, instances),
                        Err(err) => apply_error(&ui, &err),
                    }
                });
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
        } => client.create_instance(&name, &image, &memory).await,
        BackendCommand::SetState { name, action } => client.set_instance_state(&name, action).await,
        BackendCommand::DeleteInstance { name } => client.delete_instance(&name).await,
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
    {
        let tx = tx.clone();
        ui.on_refresh(move || {
            let _ = tx.send(BackendCommand::Refresh);
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_select_instance(move |idx| {
            if let Some(ui) = weak.upgrade() {
                select_row(&ui, idx);
            }
        });
    }

    {
        let tx = tx.clone();
        ui.on_create_instance(move |name, image, memory| {
            let _ = tx.send(BackendCommand::Create {
                name: name.trim().to_string(),
                image: image.to_string(),
                memory: memory.to_string(),
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
        ui.on_delete_instance(move |name| {
            let _ = tx.send(BackendCommand::DeleteInstance {
                name: name.to_string(),
            });
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
        let tx = tx.clone();
        ui.on_restore_snapshot(move |instance, snapshot| {
            let _ = tx.send(BackendCommand::RestoreSnapshot {
                instance: instance.to_string(),
                snapshot: snapshot.to_string(),
            });
        });
    }

    {
        let tx = tx.clone();
        ui.on_delete_snapshot(move |instance, snapshot| {
            let _ = tx.send(BackendCommand::DeleteSnapshot {
                instance: instance.to_string(),
                snapshot: snapshot.to_string(),
            });
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
        ui.on_open_code(move || {
            if let Err(err) = launcher::open_ide(Ide::Code)
                && let Some(ui) = weak.upgrade()
            {
                ui.set_status_message(err.to_string().into());
            }
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_open_zed(move || {
            if let Err(err) = launcher::open_ide(Ide::Zed)
                && let Some(ui) = weak.upgrade()
            {
                ui.set_status_message(err.to_string().into());
            }
        });
    }
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
    let model = ui.get_instances();
    if idx >= 0
        && let Some(row) = model.row_data(idx as usize)
    {
        ui.set_selected_index(idx);
        ui.set_selected_instance(row);
        ui.set_has_selection(true);
    } else {
        ui.set_selected_index(-1);
        ui.set_has_selection(false);
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
        is_running: inst.is_running,
        snapshots: ModelRc::from(Rc::new(VecModel::from(snaps))),
    }
}
