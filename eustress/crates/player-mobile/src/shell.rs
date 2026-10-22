//! Slint shell: gallery, search, sign-in, and the touch chrome.
//!
//! The world is Bevy's. This window is a texture on the overlay camera.
//! Touches hit Slint first for the gallery. While playing, the action layer
//! owns the thumbstick, the jump circle, and the camera drag; this file only
//! draws them.

use std::cell::RefCell;

use bevy::prelude::*;
use slint::{ModelRc, VecModel};

use crate::session::{Command, MobileApp, Mode};

slint::slint! {
    import { VerticalBox, HorizontalBox, Button, LineEdit, ScrollView } from "std-widgets.slint";

    export struct GalleryCard {
        id: string,
        title: string,
        creator: string,
        plays: string,
    }

    export component PlayerShell inherits Window {
        in-out property <string> status: "Loading the gallery…";
        in-out property <string> account: "Signed out";
        in-out property <string> server: "staging";
        in-out property <bool> playing: false;
        in-out property <bool> show-sign-in: false;
        in-out property <[GalleryCard]> cards: [];
        in-out property <float> inset-top: 0;
        in-out property <float> inset-bottom: 0;
        in-out property <float> inset-left: 0;
        in-out property <float> inset-right: 0;
        in-out property <bool> stick-visible: false;
        in-out property <float> stick-x: 0;
        in-out property <float> stick-y: 0;
        in-out property <float> knob-x: 0;
        in-out property <float> knob-y: 0;
        in-out property <float> jump-x: 0;
        in-out property <float> jump-y: 0;
        in-out property <float> jump-r: 36;

        callback search-edited(string);
        callback play-id(string);
        callback sign-in-requested();
        callback sign-in-submit(string);
        callback sign-in-cancel();
        callback back-requested();

        background: playing ? transparent : #101820;
        default-font-size: 16px;

        if !playing: VerticalBox {
            padding-top: root.inset-top * 1px + 12px;
            padding-bottom: root.inset-bottom * 1px + 12px;
            padding-left: root.inset-left * 1px + 16px;
            padding-right: root.inset-right * 1px + 16px;
            spacing: 8px;

            HorizontalBox {
                Text {
                    text: "Eustress";
                    font-size: 22px;
                    color: #f4efe6;
                    vertical-alignment: center;
                }
                Text {
                    text: server;
                    color: #9bb8c9;
                    vertical-alignment: center;
                    horizontal-alignment: right;
                }
            }
            Text {
                text: account;
                color: #c5d0d8;
            }
            Text {
                text: status;
                color: #d7c4a3;
                wrap: word-wrap;
            }

            HorizontalBox {
                spacing: 8px;
                search-box := LineEdit {
                    placeholder-text: "Search spaces";
                    edited(text) => { root.search-edited(text); }
                }
                Button {
                    text: "Sign in";
                    clicked => { root.sign-in-requested(); }
                }
            }

            if show-sign-in: VerticalBox {
                spacing: 6px;
                Text {
                    text: "Identity file path, or the toml itself. The private key stays on this device.";
                    color: #c5d0d8;
                    wrap: word-wrap;
                }
                identity-box := LineEdit {
                    placeholder-text: "eustress-username.toml";
                }
                HorizontalBox {
                    Button {
                        text: "Submit";
                        clicked => { root.sign-in-submit(identity-box.text); }
                    }
                    Button {
                        text: "Cancel";
                        clicked => { root.sign-in-cancel(); }
                    }
                }
            }

            ScrollView {
                VerticalBox {
                    spacing: 6px;
                    for card in root.cards: Rectangle {
                        background: #1c2832;
                        border-radius: 8px;
                        height: 72px;
                        VerticalBox {
                            padding: 10px;
                            spacing: 2px;
                            Text {
                                text: card.title;
                                color: #f4efe6;
                                font-size: 18px;
                            }
                            Text {
                                text: card.creator + "  ·  " + card.plays;
                                color: #9bb8c9;
                            }
                        }
                        TouchArea {
                            clicked => { root.play-id(card.id); }
                        }
                    }
                }
            }
        }

        if playing: Rectangle {
            background: transparent;

            // Always in the tree while playing, the same way the jump circle
            // is. A conditional that appears only for the frame a finger is
            // down was not drawing on the emulator.
            Rectangle {
                x: stick-x * 1px - 60px;
                y: stick-y * 1px - 60px;
                width: 120px;
                height: 120px;
                border-radius: 60px;
                background: stick-visible ? #00000055 : transparent;
                border-width: 2px;
                border-color: stick-visible ? #f4efe680 : transparent;
                Rectangle {
                    x: knob-x * 1px - stick-x * 1px + 38px;
                    y: knob-y * 1px - stick-y * 1px + 38px;
                    width: 44px;
                    height: 44px;
                    border-radius: 22px;
                    background: stick-visible ? #f4efe6cc : transparent;
                }
            }

            Rectangle {
                x: (jump-x - jump-r) * 1px;
                y: (jump-y - jump-r) * 1px;
                width: jump-r * 2px;
                height: jump-r * 2px;
                border-radius: jump-r * 1px;
                background: #f4efe633;
                border-width: 2px;
                border-color: #f4efe6aa;
                Text {
                    text: "Jump";
                    color: #f4efe6;
                    horizontal-alignment: center;
                    vertical-alignment: center;
                }
            }

            VerticalBox {
                padding-top: root.inset-top * 1px + 8px;
                padding-left: root.inset-left * 1px + 8px;
                alignment: start;
                HorizontalBox {
                    Button {
                        text: "Gallery";
                        clicked => { root.back-requested(); }
                    }
                }
                Text {
                    text: status;
                    color: #f4efe6;
                    wrap: word-wrap;
                }
            }
        }
    }
}

pub struct ShellUi {
    pub ui: PlayerShell,
}

thread_local! {
    static COMMANDS: RefCell<Vec<Command>> = const { RefCell::new(Vec::new()) };
}

pub fn take_commands() -> Vec<Command> {
    COMMANDS.with(|commands| std::mem::take(&mut *commands.borrow_mut()))
}

fn push(command: Command) {
    COMMANDS.with(|commands| commands.borrow_mut().push(command));
}

pub fn wire(ui: &PlayerShell) {
    ui.on_search_edited(move |text| push(Command::Search(text.to_string())));
    ui.on_play_id(move |id| push(Command::Play(id.to_string())));
    ui.on_sign_in_requested(move || push(Command::SignIn));
    ui.on_sign_in_submit(move |text| push(Command::SubmitIdentity(text.to_string())));
    ui.on_sign_in_cancel(move || push(Command::CancelSignIn));
    ui.on_back_requested(move || push(Command::Back));
}

/// Copy the session onto the widgets, and the safe area onto the padding.
pub fn present(world: &mut World) {
    let Some(shell) = world.get_non_send::<ShellUi>() else {
        return;
    };
    let Some(app) = world.get_resource::<MobileApp>() else {
        return;
    };
    let status = app.status.clone();
    let account = app.account.clone();
    let playing = app.mode == Mode::Playing && app.playing;
    let show_sign_in = app.show_sign_in;
    let cards: Vec<GalleryCard> = app
        .cards
        .iter()
        .map(|card| GalleryCard {
            id: card.id.clone().into(),
            title: card.name.clone().into(),
            creator: if card.creator_name.is_empty() { "Unknown".into() } else { card.creator_name.clone().into() },
            plays: format!("{} plays", card.visit_count).into(),
        })
        .collect();
    let (insets, stick, jump) = world
        .get_resource::<eustress_common::input_actions::InputState>()
        .map(|input| {
            let stick = input.stick_visual();
            let (center, radius) = eustress_common::input_actions::jump_button(input.viewport, input.insets);
            (input.insets, stick, (center, radius))
        })
        .unwrap_or_default();

    let server = eustress_common::api_base::api_label()
        .map(|(name, host)| format!("{name} · {host}"))
        .unwrap_or_else(|| "production".into());

    shell.ui.set_status(status.into());
    shell.ui.set_account(account.into());
    shell.ui.set_server(server.into());
    shell.ui.set_playing(playing);
    shell.ui.set_show_sign_in(show_sign_in);
    shell.ui.set_cards(ModelRc::new(VecModel::from(cards)));
    shell.ui.set_inset_top(insets.top);
    shell.ui.set_inset_bottom(insets.bottom);
    shell.ui.set_inset_left(insets.left);
    shell.ui.set_inset_right(insets.right);
    let viewport = world
        .get_resource::<eustress_common::input_actions::InputState>()
        .map(|input| input.viewport)
        .unwrap_or(Vec2::new(1280.0, 720.0));
    let rest = eustress_common::input_actions::thumbstick_rest(viewport, insets);
    if let Some((origin, knob)) = stick {
        // Positions before visibility, so the frame the ring appears is
        // already at this finger and not the previous drag's offset.
        shell.ui.set_stick_x(origin.x);
        shell.ui.set_stick_y(origin.y);
        shell.ui.set_knob_x(knob.x);
        shell.ui.set_knob_y(knob.y);
    } else {
        // No finger: the ring sits in the lower left, knob centered, the
        // same corner the jump circle uses on the right.
        shell.ui.set_stick_x(rest.x);
        shell.ui.set_stick_y(rest.y);
        shell.ui.set_knob_x(rest.x);
        shell.ui.set_knob_y(rest.y);
    }
    shell.ui.set_stick_visible(playing);
    shell.ui.set_jump_x(jump.0.x);
    shell.ui.set_jump_y(jump.0.y);
    shell.ui.set_jump_r(jump.1);
}
