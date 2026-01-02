// =============================================================================
// Eustress Web - Form Components
// =============================================================================
// Table of Contents:
// 1. TextInput
// 2. TextArea
// 3. Checkbox
// 4. Select
// =============================================================================

use leptos::prelude::*;

// -----------------------------------------------------------------------------
// 1. TextInput
// -----------------------------------------------------------------------------

/// Text input field with label and error state.
#[component]
pub fn TextInput(
    #[prop(into)] label: String,
    #[prop(into)] value: RwSignal<String>,
    #[prop(optional, into)] placeholder: String,
    #[prop(optional, into)] input_type: String,
    #[prop(optional, into)] error: Option<String>,
    #[prop(optional)] disabled: bool,
    #[prop(optional)] required: bool,
) -> impl IntoView {
    let input_type = if input_type.is_empty() { "text".to_string() } else { input_type };
    let has_error = error.is_some();
    
    view! {
        <div class="form-field" class:has-error=has_error>
            <label class="form-label">
                {label}
                {required.then(|| view! { <span class="required">"*"</span> })}
            </label>
            <input
                type=input_type
                class="form-input"
                placeholder=placeholder
                disabled=disabled
                required=required
                prop:value=move || value.get()
                on:input=move |e| {
                    value.set(event_target_value(&e));
                }
            />
            {error.map(|e| view! { <span class="form-error">{e}</span> })}
        </div>
    }
}

// -----------------------------------------------------------------------------
// 2. TextArea
// -----------------------------------------------------------------------------

/// Multi-line text area.
#[component]
pub fn TextArea(
    #[prop(into)] label: String,
    #[prop(into)] value: RwSignal<String>,
    #[prop(optional, into)] placeholder: String,
    #[prop(optional)] rows: u32,
    #[prop(optional, into)] error: Option<String>,
    #[prop(optional)] disabled: bool,
) -> impl IntoView {
    let rows = if rows == 0 { 4 } else { rows };
    let has_error = error.is_some();
    
    view! {
        <div class="form-field" class:has-error=has_error>
            <label class="form-label">{label}</label>
            <textarea
                class="form-textarea"
                placeholder=placeholder
                rows=rows
                disabled=disabled
                prop:value=move || value.get()
                on:input=move |e| {
                    value.set(event_target_value(&e));
                }
            />
            {error.map(|e| view! { <span class="form-error">{e}</span> })}
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. Checkbox
// -----------------------------------------------------------------------------

/// Checkbox input with label.
#[component]
pub fn Checkbox(
    #[prop(into)] label: String,
    #[prop(into)] checked: RwSignal<bool>,
    #[prop(optional)] disabled: bool,
) -> impl IntoView {
    view! {
        <label class="form-checkbox">
            <input
                type="checkbox"
                disabled=disabled
                prop:checked=move || checked.get()
                on:change=move |e| {
                    checked.set(event_target_checked(&e));
                }
            />
            <span class="checkbox-label">{label}</span>
        </label>
    }
}

// -----------------------------------------------------------------------------
// 4. Select
// -----------------------------------------------------------------------------

/// Select dropdown option.
#[derive(Clone)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

/// Select dropdown component.
#[component]
pub fn Select(
    #[prop(into)] label: String,
    #[prop(into)] value: RwSignal<String>,
    #[prop(into)] options: Vec<SelectOption>,
    #[prop(optional)] disabled: bool,
) -> impl IntoView {
    view! {
        <div class="form-field">
            <label class="form-label">{label}</label>
            <select
                class="form-select"
                disabled=disabled
                prop:value=move || value.get()
                on:change=move |e| {
                    value.set(event_target_value(&e));
                }
            >
                <For
                    each=move || options.clone()
                    key=|opt| opt.value.clone()
                    children=move |opt| {
                        view! {
                            <option value=opt.value.clone()>{opt.label}</option>
                        }
                    }
                />
            </select>
        </div>
    }
}
