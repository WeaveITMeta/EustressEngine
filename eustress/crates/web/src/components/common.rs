// =============================================================================
// Eustress Web - Common UI Components
// =============================================================================
// Table of Contents:
// 1. Button
// 2. Card
// 3. Loading Spinner
// 4. Error Display
// 5. Modal
// =============================================================================

use leptos::prelude::*;

// -----------------------------------------------------------------------------
// 1. Button
// -----------------------------------------------------------------------------

/// Button variant styles.
#[derive(Clone, Copy, Default, PartialEq)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Danger,
    Ghost,
}

impl ButtonVariant {
    fn class(&self) -> &'static str {
        match self {
            ButtonVariant::Primary => "btn btn-primary",
            ButtonVariant::Secondary => "btn btn-secondary",
            ButtonVariant::Danger => "btn btn-danger",
            ButtonVariant::Ghost => "btn btn-ghost",
        }
    }
}

/// Reusable button component.
#[component]
pub fn Button(
    #[prop(into)] label: String,
    #[prop(optional)] variant: ButtonVariant,
    #[prop(optional)] disabled: bool,
    #[prop(optional)] loading: bool,
    #[prop(optional, into)] on_click: Option<Callback<()>>,
) -> impl IntoView {
    let handle_click = move |_| {
        if let Some(callback) = &on_click {
            callback.run(());
        }
    };
    
    view! {
        <button
            class=variant.class()
            disabled=disabled || loading
            on:click=handle_click
        >
            {move || if loading {
                view! { <span class="spinner-small"></span> }.into_any()
            } else {
                view! { <span>{label.clone()}</span> }.into_any()
            }}
        </button>
    }
}

// -----------------------------------------------------------------------------
// 2. Card
// -----------------------------------------------------------------------------

/// Card container component.
#[component]
pub fn Card(
    #[prop(optional, into)] title: Option<String>,
    #[prop(optional, into)] class: String,
    children: Children,
) -> impl IntoView {
    view! {
        <div class=format!("card {}", class)>
            {title.map(|t| view! {
                <div class="card-header">
                    <h3 class="card-title">{t}</h3>
                </div>
            })}
            <div class="card-body">
                {children()}
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. Loading Spinner
// -----------------------------------------------------------------------------

/// Full-page loading spinner.
#[component]
pub fn LoadingSpinner(#[prop(optional, into)] message: Option<String>) -> impl IntoView {
    view! {
        <div class="loading-container">
            <div class="spinner"></div>
            {message.map(|m| view! { <p class="loading-message">{m}</p> })}
        </div>
    }
}

/// Inline loading indicator.
#[component]
pub fn InlineLoader() -> impl IntoView {
    view! {
        <span class="spinner-small"></span>
    }
}

// -----------------------------------------------------------------------------
// 4. Error Display
// -----------------------------------------------------------------------------

/// Error message display.
#[component]
pub fn ErrorDisplay(
    #[prop(into)] message: String,
    #[prop(optional, into)] on_dismiss: Option<Callback<()>>,
) -> impl IntoView {
    view! {
        <div class="error-display">
            <span class="error-icon">"⚠️"</span>
            <span class="error-message">{message}</span>
            {on_dismiss.map(|dismiss| view! {
                <button class="error-dismiss" on:click=move |_| dismiss.run(())>
                    "✕"
                </button>
            })}
        </div>
    }
}

// -----------------------------------------------------------------------------
// 5. Modal
// -----------------------------------------------------------------------------

// Note: Modal with Children is complex in Leptos 0.7 CSR mode.
// For now, use inline modal markup in pages that need modals.
