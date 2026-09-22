use crate::api::ApiClient;
use eustress_avatar_schema::{AvatarDescriptor, AvatarIdentity, Norm01, RigDefinition};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;

#[derive(Deserialize)]
struct AvatarResponse {
    descriptor: Option<AvatarDescriptor>,
    /// The one identity this account may have: `"M"` or `"F"` from the sex
    /// marker on a human's verified document, `"R"` for an agent account, or
    /// `None` when a human has nothing usable on file (an X marker, an
    /// unreadable field, or a verification that predates the field). Absent
    /// from older server builds, so it defaults rather than failing the parse.
    #[serde(default)]
    locked_identity: Option<String>,
    /// `"document"` or `"agent"`, so the panel can say why the identity is fixed.
    #[serde(default)]
    lock_source: Option<String>,
}

/// A mounted customizer belongs to the authenticated profile owner only.
#[component]
pub fn AvatarCustomizer() -> impl IntoView {
    let avatar = RwSignal::new(AvatarDescriptor::default());
    let rigs = RwSignal::new(AvatarIdentity::ALL.map(RigDefinition::builtin).to_vec());
    // The account's one identity. Never a choice: a human is the sex marker on
    // their verified document, an agent account is Robot. `None` means a human
    // has no usable marker on file and the controls do not render.
    let locked = RwSignal::new(Option::<AvatarIdentity>::None);
    let lock_source = RwSignal::new(String::new());
    let blocked = RwSignal::new(false);
    // The rig <select>'s value is applied from an Effect, not `prop:value`.
    // `prop:value` runs when the element is created, before the reactive
    // <option> list has mounted, so it set a value with no matching option and
    // the browser resolved that to empty; the catalog load then re-rendered the
    // options and nothing re-applied it. An Effect runs after the DOM commits,
    // and reading `rigs` here makes it re-run whenever the option list changes.
    let rig_select = NodeRef::<leptos::html::Select>::new();
    Effect::new(move |_| {
        let id = avatar.get().resolved_rig().id;
        rigs.track();
        if let Some(select) = rig_select.get() {
            select.set_value(&id);
        }
    });
    let status = RwSignal::new(String::new());
    let loading = RwSignal::new(true);
    let saving = RwSignal::new(false);
    let animation = RwSignal::new("Idle".to_string());
    spawn_local(async move {
        match ApiClient::new("https://api.eustress.dev")
            .get::<AvatarResponse>("/api/avatar")
            .await
        {
            Ok(response) => {
                let fixed = response.locked_identity.as_deref().and_then(AvatarIdentity::from_code);
                locked.set(fixed);
                lock_source.set(response.lock_source.unwrap_or_default());
                let Some(fixed) = fixed else {
                    // No marker to bind to. The server refuses writes in this
                    // state, so offering controls would only produce errors.
                    blocked.set(true);
                    loading.set(false);
                    return;
                };
                match response.descriptor {
                    Some(mut descriptor) => {
                        let identity = descriptor.resolved_identity();
                        descriptor.identity = identity;
                        descriptor.schema_version = eustress_avatar_schema::AVATAR_SCHEMA_VERSION;
                        // A saved avatar that disagrees with the account's identity
                        // is brought back to it. The server would refuse to store it
                        // as it was, so correcting it here is the difference between
                        // a working Save and a dead one.
                        if identity != fixed {
                            descriptor.select_identity(fixed);
                            status.set("Your avatar was updated to match your verified identity.".into());
                        } else if fixed == AvatarIdentity::Robot && !descriptor.morphs.is_robot_body() {
                            // Robot bodies do not resize. The server refuses any
                            // other height or build, so a drifted save is pinned
                            // back here rather than left as a Save that can only fail.
                            descriptor.morphs = eustress_avatar_schema::BodyMorphs::robot();
                            status.set("Robot bodies have a fixed size; yours was reset.".into());
                        }
                        match descriptor.validate() {
                            Ok(()) => {
                                if let Some(rig) = descriptor.rig.clone() {
                                    rigs.update(|catalog| {
                                        if !catalog.iter().any(|r| r.id == rig.id) {
                                            catalog.push(rig);
                                        }
                                    });
                                }
                                avatar.set(descriptor);
                            }
                            Err(error) => status.set(format!("Saved avatar could not load: {error}")),
                        }
                    }
                    // A fresh account starts as its document says, not as the
                    // schema's default.
                    None => avatar.update(|d| d.select_identity(fixed)),
                }
            }
            Err(error) => status.set(format!("Could not load saved avatar: {error}")),
        }
        loading.set(false);
    });
    spawn_local(async move {
        if let Ok(response) = gloo_net::http::Request::get("/assets/characters/rigs.json")
            .send()
            .await
        {
            if let Ok(catalog) = response.json::<Vec<RigDefinition>>().await {
                let mut valid = rigs.get_untracked();
                for rig in catalog {
                    if rig.validate().is_ok() && !valid.iter().any(|r| r.id == rig.id) {
                        valid.push(rig);
                    }
                }
                rigs.set(valid);
            }
        }
    });
    let save = move |_| {
        let descriptor = avatar.get_untracked();
        if let Err(error) = descriptor.validate() {
            status.set(error);
            return;
        }
        saving.set(true);
        status.set(String::new());
        spawn_local(async move {
            let result = ApiClient::new("https://api.eustress.dev")
                .put::<AvatarResponse, _>("/api/avatar", &descriptor)
                .await;
            match result {
                Ok(_) => status.set("Avatar saved to your account.".into()),
                Err(error) => status.set(format!("Avatar was not saved: {error}")),
            }
            saving.set(false);
        });
    };
    view! {
        <Show when=move || !blocked.get() fallback=move || view! {
            <div class="avatar-customizer avatar-customizer-blocked">
                <h3 class="avatar-section-title">"Customize Character"</h3>
                <p>"Your avatar is tied to the sex marker on your verified identity document, and your current verification does not carry one we can read."</p>
                <p>"Verify again with a document that shows the marker, and this panel unlocks."</p>
                <a class="btn btn-primary" href="/verify">"Verify identity"</a>
            </div>
        }>
        <div class="avatar-customizer">
            <div class="avatar-preview-section">
                <div class="avatar-viewport">
                    <model-viewer
                        src=move || avatar.get().resolved_rig().body_asset.replace("bundled://", "/assets/")
                        alt=move || format!("{} animated avatar preview", avatar.get().resolved_rig().label)
                        animation-name=move || animation.get()
                        scale=move || {
                            let d=avatar.get();
                            let scale=d.morphs.height.remap(eustress_avatar_schema::MIN_HEIGHT_M,eustress_avatar_schema::MAX_HEIGHT_M)/eustress_avatar_schema::NOMINAL_BIND_HEIGHT_M;
                            let width=1.0+0.25*(d.morphs.build.get()*2.0-1.0);
                            format!("{} {} {}",scale*width,scale,scale*width)
                        }
                        autoplay="" camera-controls="" shadow-intensity="1"
                        on:error=move |_| status.set("The selected rig preview could not load. Check that its assets are installed.".into())
                        camera-orbit="15deg 80deg 4.5m" field-of-view="30deg"
                        style="width:100%;height:520px;min-height:350px;background:#14191f;"
                    ></model-viewer>
                    <div class="avatar-rotate-hint">"Drag to rotate · Scroll to zoom"</div>
                </div>
                <label for="avatar-animation">"Preview animation"</label>
                <select id="avatar-animation" class="form-input avatar-select"
                    prop:value=move || animation.get() on:change=move |ev| animation.set(event_target_value(&ev))>
                    <option value="Idle">"Idle"</option><option value="Walk">"Walk"</option>
                    <option value="Run">"Run"</option><option value="Jump">"Jump"</option>
                </select>
            </div>
            <div class="avatar-controls-section">
                <h3 class="avatar-section-title">"Customize Character"</h3>
                <fieldset disabled=move || loading.get() || saving.get() style="border:0;padding:0;margin:0;">
                    <div class="avatar-category">
                        <div class="avatar-option">
                            <label for="avatar-identity">"Identity"</label>
                            <output id="avatar-identity" class="form-input avatar-select avatar-identity-fixed" aria-readonly="true">
                                {move || locked.get().map(|i| i.label()).unwrap_or("")}
                            </output>
                            <p class="avatar-hint">{move || match lock_source.get().as_str() {
                                "agent" => "Agent accounts are Robot.",
                                _ => "Set by the sex marker on your verified identity document.",
                            }}</p>
                        </div>
                        <div class="avatar-option">
                            <label for="avatar-rig">"Character rig"</label>
                            <select id="avatar-rig" class="form-input avatar-select" node_ref=rig_select
                                on:change=move |ev| {
                                    let id=event_target_value(&ev);
                                    if let Some(rig)=rigs.get_untracked().into_iter().find(|r| r.id==id && r.identity==avatar.get_untracked().resolved_identity()) {
                                        avatar.update(|d| d.rig=Some(rig)); status.set(String::new());
                                    }
                                }>
                                {move || {
                                    let d=avatar.get();let mut available=rigs.get().into_iter().filter(|r| r.identity==d.resolved_identity()).collect::<Vec<_>>();
                                    if let Some(r)=d.rig {if !available.iter().any(|v| v.id==r.id){available.push(r);}}
                                    let selected=avatar.get().resolved_rig().id;
                                    available.into_iter().map(|r| {let is_selected=r.id==selected;view! {<option value=r.id selected=is_selected>{r.label}</option>}}).collect_view()
                                }}
                            </select>
                        </div>
                    </div>
                    // Robots have one body: the authored model at its bind size.
                    // Agents do not resize, so the sliders are not offered at all
                    // rather than shown disabled.
                    <Show when=move || locked.get() != Some(AvatarIdentity::Robot)>
                        <div class="avatar-category">
                            <h4 class="avatar-category-title">"Body"</h4>
                            <div class="avatar-option">
                                <label for="avatar-height">"Height"</label>
                                <input id="avatar-height" type="range" min="0" max="100" class="avatar-slider"
                                    prop:value=move || avatar.get().morphs.height.percent()
                                    on:input=move |ev| {if let Ok(v)=event_target_value(&ev).parse(){avatar.update(|d| d.morphs.height=Norm01::from_percent(v));}} />
                            </div>
                            <div class="avatar-option">
                                <label for="avatar-build">"Build"</label>
                                <input id="avatar-build" type="range" min="0" max="100" class="avatar-slider"
                                    prop:value=move || avatar.get().morphs.build.percent()
                                    on:input=move |ev| {if let Ok(v)=event_target_value(&ev).parse(){avatar.update(|d| d.morphs.build=Norm01::from_percent(v));}} />
                            </div>
                        </div>
                    </Show>
                    <button class="btn btn-primary avatar-save-btn" on:click=save>
                        {move || if loading.get(){"Loading…"}else if saving.get(){"Saving…"}else{"Save Avatar"}}
                    </button>
                </fieldset>
                <p role="status" aria-live="polite">{move || status.get()}</p>
                <a class="btn" download="avatar.json" href=move || {
                    let json=serde_json::to_string_pretty(&avatar.get()).unwrap_or_default();
                    format!("data:application/json;charset=utf-8,{}",urlencoding::encode(&json))
                }>"Export avatar"</a>
            </div>
        </div>
        </Show>
    }
}
