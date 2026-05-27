use egui::{Color32, Context, RichText, Window};

use crate::ui::password_field::MaskedBuffer;

pub struct MasterPasswordPromptUi {
    pub mode: MasterPasswordMode,
    pub password: String,
    pub confirm: String,
    pub error: Option<String>,
    pub cancellable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasterPasswordMode {
    Unlock,
    Create,
}

#[derive(Debug, Clone)]
pub enum MasterPasswordResult {
    Pending,
    Submit(String),
    Cancelled,
}

impl MasterPasswordPromptUi {
    pub fn new(mode: MasterPasswordMode) -> Self {
        Self {
            mode,
            password: String::new(),
            confirm: String::new(),
            error: None,
            cancellable: false,
        }
    }

    pub fn for_reauth() -> Self {
        Self {
            mode: MasterPasswordMode::Unlock,
            password: String::new(),
            confirm: String::new(),
            error: None,
            cancellable: true,
        }
    }

    pub fn show(&mut self, ctx: &Context) -> MasterPasswordResult {
        let mut result = MasterPasswordResult::Pending;

        let title = if self.cancellable {
            "Confirm master password"
        } else {
            match self.mode {
                MasterPasswordMode::Unlock => "Unlock secrets",
                MasterPasswordMode::Create => "Set master password",
            }
        };

        let mut open = true;
        let mut window = Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]);
        if self.cancellable {
            window = window.open(&mut open);
        }
        window.show(ctx, |ui| {
                ui.set_min_width(420.0);

                match self.mode {
                    MasterPasswordMode::Unlock if self.cancellable => {
                        ui.label("Enter your master password to reveal the saved credential.");
                    }
                    MasterPasswordMode::Unlock => {
                        ui.label("Enter the master password to decrypt your saved credentials.");
                    }
                    MasterPasswordMode::Create => {
                        ui.label(
                            "Choose a master password. It encrypts every saved password and key passphrase. \
                             It cannot be recovered if you forget it.",
                        );
                    }
                }

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Password:");
                    let mut buf = MaskedBuffer::new(&mut self.password);
                    ui.add(
                        egui::TextEdit::singleline(&mut buf)
                            .desired_width(260.0),
                    );
                });

                if matches!(self.mode, MasterPasswordMode::Create) {
                    ui.horizontal(|ui| {
                        ui.label("Confirm: ");
                        let mut buf = MaskedBuffer::new(&mut self.confirm);
                        ui.add(
                            egui::TextEdit::singleline(&mut buf)
                                .desired_width(260.0),
                        );
                    });
                }

                if let Some(err) = &self.error {
                    ui.add_space(6.0);
                    ui.colored_label(Color32::from_rgb(220, 80, 80), RichText::new(err).strong());
                }

                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    let label = match self.mode {
                        MasterPasswordMode::Unlock => "Unlock",
                        MasterPasswordMode::Create => "Set password",
                    };
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let clicked = ui.button(label).clicked();
                    if clicked || enter {
                        if let Some(err) = self.validate() {
                            self.error = Some(err);
                        } else {
                            result = MasterPasswordResult::Submit(self.password.clone());
                        }
                    }
                    if self.cancellable {
                        if ui.button("Cancel").clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            result = MasterPasswordResult::Cancelled;
                        }
                    }
                });
            });

        if self.cancellable && !open && matches!(result, MasterPasswordResult::Pending) {
            result = MasterPasswordResult::Cancelled;
        }

        result
    }

    fn validate(&self) -> Option<String> {
        if self.password.is_empty() {
            return Some("Password cannot be empty.".to_string());
        }
        if matches!(self.mode, MasterPasswordMode::Create) {
            if self.password.len() < 8 {
                return Some("Use at least 8 characters.".to_string());
            }
            if self.password != self.confirm {
                return Some("Passwords do not match.".to_string());
            }
        }
        None
    }
}
