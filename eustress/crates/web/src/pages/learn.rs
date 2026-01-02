// =============================================================================
// Eustress Web - Learn Page (Industrial Design)
// =============================================================================
// Documentation, tutorials, and resources for building Eustress spaces
// Features MindSpace as the premiere 3D learning tool
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// Documentation category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocCategory {
    GettingStarted,
    SoulScript,
    Scripting,
    Building,
    Physics,
    Networking,
    Audio,
    UI,
    Publishing,
}

impl DocCategory {
    fn as_str(&self) -> &'static str {
        match self {
            Self::GettingStarted => "getting-started",
            Self::SoulScript => "soulscript",
            Self::Scripting => "scripting",
            Self::Building => "building",
            Self::Physics => "physics",
            Self::Networking => "networking",
            Self::Audio => "audio",
            Self::UI => "ui",
            Self::Publishing => "publishing",
        }
    }
    
    fn display_name(&self) -> &'static str {
        match self {
            Self::GettingStarted => "Getting Started",
            Self::SoulScript => "SoulScript",
            Self::Scripting => "Scripting",
            Self::Building => "Building",
            Self::Physics => "Physics",
            Self::Networking => "Networking",
            Self::Audio => "Audio",
            Self::UI => "UI Systems",
            Self::Publishing => "Publishing",
        }
    }
    
    fn icon_path(&self) -> &'static str {
        match self {
            Self::GettingStarted => "/assets/icons/rocket.svg",
            Self::SoulScript => "/assets/icons/brain.svg",
            Self::Scripting => "/assets/icons/code.svg",
            Self::Building => "/assets/icons/cube.svg",
            Self::Physics => "/assets/icons/physics.svg",
            Self::Networking => "/assets/icons/network.svg",
            Self::Audio => "/assets/icons/audio.svg",
            Self::UI => "/assets/icons/template.svg",
            Self::Publishing => "/assets/icons/upload.svg",
        }
    }
    
    fn description(&self) -> &'static str {
        match self {
            Self::GettingStarted => "Set up your environment and create your first place",
            Self::SoulScript => "Write natural language descriptions that become 3D experiences",
            Self::Scripting => "Learn to code behavior with Soul and the Eustress API",
            Self::Building => "Master 3D modeling, terrain, and level design",
            Self::Physics => "Implement realistic physics and collisions",
            Self::Networking => "Build multiplayer experiences with real-time sync",
            Self::Audio => "Add music, sound effects, and spatial audio",
            Self::UI => "Create menus, HUDs, and interactive interfaces",
            Self::Publishing => "Deploy your place and reach players worldwide",
        }
    }
}

/// Tutorial item.
#[derive(Clone, Debug, PartialEq)]
pub struct Tutorial {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: DocCategory,
    pub duration: String,
    pub difficulty: String,
    pub is_video: bool,
}

/// Resource link.
#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub title: String,
    pub description: String,
    pub url: String,
    pub icon: String,
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Learn page - documentation, tutorials, and resources.
#[component]
pub fn LearnPage() -> impl IntoView {
    let selected_category = RwSignal::new("all".to_string());
    
    // Sample tutorials
    let tutorials = vec![
        Tutorial {
            id: "1".to_string(),
            title: "Your First Place".to_string(),
            description: "Create a simple interactive environment from scratch".to_string(),
            category: DocCategory::GettingStarted,
            duration: "15 min".to_string(),
            difficulty: "Beginner".to_string(),
            is_video: true,
        },
        Tutorial {
            id: "2".to_string(),
            title: "Introduction to Scripting".to_string(),
            description: "Learn the basics of Lua scripting in Eustress".to_string(),
            category: DocCategory::Scripting,
            duration: "25 min".to_string(),
            difficulty: "Beginner".to_string(),
            is_video: true,
        },
        Tutorial {
            id: "3".to_string(),
            title: "Building with Parts".to_string(),
            description: "Master the fundamentals of 3D construction".to_string(),
            category: DocCategory::Building,
            duration: "20 min".to_string(),
            difficulty: "Beginner".to_string(),
            is_video: false,
        },
        Tutorial {
            id: "4".to_string(),
            title: "Physics Simulations".to_string(),
            description: "Create realistic physics-based interactions".to_string(),
            category: DocCategory::Physics,
            duration: "30 min".to_string(),
            difficulty: "Intermediate".to_string(),
            is_video: true,
        },
        Tutorial {
            id: "5".to_string(),
            title: "Multiplayer Basics".to_string(),
            description: "Sync player data and create shared experiences".to_string(),
            category: DocCategory::Networking,
            duration: "35 min".to_string(),
            difficulty: "Intermediate".to_string(),
            is_video: true,
        },
        Tutorial {
            id: "6".to_string(),
            title: "Custom UI Design".to_string(),
            description: "Build beautiful interfaces with the UI system".to_string(),
            category: DocCategory::UI,
            duration: "25 min".to_string(),
            difficulty: "Intermediate".to_string(),
            is_video: false,
        },
        // SoulScript Tutorials
        Tutorial {
            id: "7".to_string(),
            title: "Introduction to SoulScript".to_string(),
            description: "Write natural language descriptions that become 3D worlds".to_string(),
            category: DocCategory::SoulScript,
            duration: "10 min".to_string(),
            difficulty: "Beginner".to_string(),
            is_video: true,
        },
        Tutorial {
            id: "8".to_string(),
            title: "SoulScript API Reference".to_string(),
            description: "Complete guide to spawning entities, physics, and animations".to_string(),
            category: DocCategory::SoulScript,
            duration: "30 min".to_string(),
            difficulty: "Intermediate".to_string(),
            is_video: false,
        },
        Tutorial {
            id: "9".to_string(),
            title: "Building a Solar System".to_string(),
            description: "Create an animated solar system with orbiting planets using SoulScript".to_string(),
            category: DocCategory::SoulScript,
            duration: "20 min".to_string(),
            difficulty: "Intermediate".to_string(),
            is_video: true,
        },
    ];
    
    // Filter tutorials
    let filtered_tutorials = {
        let tutorials = tutorials.clone();
        move || {
            let cat = selected_category.get();
            if cat == "all" {
                tutorials.clone()
            } else {
                tutorials.iter()
                    .filter(|t| t.category.as_str() == cat)
                    .cloned()
                    .collect()
            }
        }
    };
    
    // Resources
    let resources = vec![
        Resource {
            title: "API Reference".to_string(),
            description: "Complete documentation of all Eustress APIs".to_string(),
            url: "/docs/api".to_string(),
            icon: "/assets/icons/book.svg".to_string(),
        },
        Resource {
            title: "Community Forums".to_string(),
            description: "Get help and share knowledge with other creators".to_string(),
            url: "/community".to_string(),
            icon: "/assets/icons/users.svg".to_string(),
        },
        Resource {
            title: "Sample Projects".to_string(),
            description: "Download and learn from complete example spaces".to_string(),
            url: "/samples".to_string(),
            icon: "/assets/icons/folder.svg".to_string(),
        },
    ];
    
    view! {
        <div class="page page-learn-industrial">
            <CentralNav active="learn".to_string() />
            
            // Background
            <div class="learn-bg">
                <div class="learn-grid-overlay"></div>
                <div class="learn-glow glow-1"></div>
                <div class="learn-glow glow-2"></div>
            </div>
            
            // Hero Section
            <section class="learn-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"LEARN"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="learn-title">"Master Eustress"</h1>
                <p class="learn-subtitle">"Everything you need to build amazing 3D experiences"</p>
            </section>
            
            // MindSpace Feature Section
            <section class="mindspace-feature">
                <div class="mindspace-card">
                    <div class="mindspace-visual">
                        <div class="mindspace-orb">
                            <img src="/assets/icons/brain.svg" alt="MindSpace" class="mindspace-icon" />
                        </div>
                        <div class="mindspace-rings">
                            <div class="ring ring-1"></div>
                            <div class="ring ring-2"></div>
                            <div class="ring ring-3"></div>
                        </div>
                    </div>
                    <div class="mindspace-content">
                        <div class="mindspace-badge">"FEATURED"</div>
                        <h2 class="mindspace-title">"MindSpace"</h2>
                        <p class="mindspace-tagline">"The premiere way to mind map and learn in 3D"</p>
                        <p class="mindspace-description">
                            "Visualize concepts, connect ideas, and explore knowledge in an immersive 3D environment. 
                            MindSpace transforms how you learn by letting you build spatial relationships between topics, 
                            creating memorable mental models that stick."
                        </p>
                        <ul class="mindspace-features">
                            <li>
                                <img src="/assets/icons/check.svg" alt="Check" />
                                "3D mind mapping with infinite canvas"
                            </li>
                            <li>
                                <img src="/assets/icons/check.svg" alt="Check" />
                                "Collaborative learning spaces"
                            </li>
                            <li>
                                <img src="/assets/icons/check.svg" alt="Check" />
                                "VR/AR support for immersive study"
                            </li>
                            <li>
                                <img src="/assets/icons/check.svg" alt="Check" />
                                "AI-powered concept connections"
                            </li>
                        </ul>
                        <div class="mindspace-actions">
                            <a href="/mindspace" class="btn-mindspace primary">
                                <img src="/assets/icons/play.svg" alt="Launch" />
                                "Launch MindSpace"
                            </a>
                            <a href="/docs/mindspace" class="btn-mindspace secondary">
                                <img src="/assets/icons/book.svg" alt="Docs" />
                                "Learn More"
                            </a>
                        </div>
                    </div>
                </div>
            </section>
            
            // Documentation Categories
            <section class="docs-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/book.svg" alt="Docs" class="section-icon" />
                    <h2>"Documentation"</h2>
                </div>
                
                <div class="docs-grid">
                    <a href="/docs/getting-started" class="doc-card">
                        <img src="/assets/icons/rocket.svg" alt="Getting Started" class="doc-icon" />
                        <h3>"Getting Started"</h3>
                        <p>"Set up your environment and create your first place"</p>
                    </a>
                    <a href="/docs/scripting" class="doc-card">
                        <img src="/assets/icons/code.svg" alt="Scripting" class="doc-icon" />
                        <h3>"Scripting"</h3>
                        <p>"Learn to code behavior with Soul and the Eustress API"</p>
                    </a>
                    <a href="/docs/building" class="doc-card">
                        <img src="/assets/icons/cube.svg" alt="Building" class="doc-icon" />
                        <h3>"Building"</h3>
                        <p>"Master 3D modeling, terrain, and level design"</p>
                    </a>
                    <a href="/docs/physics" class="doc-card">
                        <img src="/assets/icons/physics.svg" alt="Physics" class="doc-icon" />
                        <h3>"Physics"</h3>
                        <p>"Implement realistic physics and collisions"</p>
                    </a>
                    <a href="/docs/networking" class="doc-card">
                        <img src="/assets/icons/network.svg" alt="Networking" class="doc-icon" />
                        <h3>"Networking"</h3>
                        <p>"Build multiplayer experiences with real-time sync"</p>
                    </a>
                    <a href="/docs/audio" class="doc-card">
                        <img src="/assets/icons/audio.svg" alt="Audio" class="doc-icon" />
                        <h3>"Audio"</h3>
                        <p>"Add music, sound effects, and spatial audio"</p>
                    </a>
                    <a href="/docs/ui" class="doc-card">
                        <img src="/assets/icons/template.svg" alt="UI" class="doc-icon" />
                        <h3>"UI Systems"</h3>
                        <p>"Create menus, HUDs, and interactive interfaces"</p>
                    </a>
                    <a href="/docs/publishing" class="doc-card">
                        <img src="/assets/icons/upload.svg" alt="Publishing" class="doc-icon" />
                        <h3>"Publishing"</h3>
                        <p>"Deploy your place and reach players worldwide"</p>
                    </a>
                    <a href="/docs/earning" class="doc-card">
                        <img src="/assets/icons/trending.svg" alt="Earning" class="doc-icon" />
                        <h3>"Earning"</h3>
                        <p>"Monetize your creations and earn Bliss revenue"</p>
                    </a>
                </div>
            </section>
            
            // Tutorials Section
            <section class="tutorials-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/play.svg" alt="Tutorials" class="section-icon" />
                    <h2>"Tutorials"</h2>
                </div>
                
                <div class="tutorial-filters">
                    <button 
                        class="chip"
                        class:active=move || selected_category.get() == "all"
                        on:click=move |_| selected_category.set("all".to_string())
                    >"All"</button>
                    <button 
                        class="chip"
                        class:active=move || selected_category.get() == "getting-started"
                        on:click=move |_| selected_category.set("getting-started".to_string())
                    >"Getting Started"</button>
                    <button 
                        class="chip"
                        class:active=move || selected_category.get() == "scripting"
                        on:click=move |_| selected_category.set("scripting".to_string())
                    >"Scripting"</button>
                    <button 
                        class="chip"
                        class:active=move || selected_category.get() == "building"
                        on:click=move |_| selected_category.set("building".to_string())
                    >"Building"</button>
                    <button 
                        class="chip"
                        class:active=move || selected_category.get() == "physics"
                        on:click=move |_| selected_category.set("physics".to_string())
                    >"Physics"</button>
                </div>
                
                <div class="tutorials-grid">
                    <For
                        each=filtered_tutorials
                        key=|t| t.id.clone()
                        children=move |tutorial| {
                            let url = format!("/learn/{}", tutorial.id);
                            view! {
                                <a href=url class="tutorial-card">
                                    <div class="tutorial-thumbnail">
                                        {if tutorial.is_video {
                                            view! {
                                                <img src="/assets/icons/play.svg" alt="Video" class="play-badge" />
                                            }.into_any()
                                        } else {
                                            view! {
                                                <img src="/assets/icons/book.svg" alt="Article" class="play-badge" />
                                            }.into_any()
                                        }}
                                    </div>
                                    <div class="tutorial-content">
                                        <span class="tutorial-category">{tutorial.category.display_name()}</span>
                                        <h3 class="tutorial-title">{tutorial.title}</h3>
                                        <p class="tutorial-desc">{tutorial.description}</p>
                                        <div class="tutorial-meta">
                                            <span class="meta-item">
                                                <img src="/assets/icons/clock.svg" alt="Duration" />
                                                {tutorial.duration}
                                            </span>
                                            <span class="difficulty-badge">{tutorial.difficulty}</span>
                                        </div>
                                    </div>
                                </a>
                            }
                        }
                    />
                </div>
                
                <div class="view-all-link">
                    <a href="/tutorials" class="btn-view-all">
                        "View All Tutorials"
                        <img src="/assets/icons/arrow-right.svg" alt="Arrow" />
                    </a>
                </div>
            </section>
            
            // Resources Section
            <section class="resources-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/sparkles.svg" alt="Resources" class="section-icon" />
                    <h2>"Resources"</h2>
                </div>
                
                <div class="resources-grid">
                    {resources.into_iter().map(|resource| {
                        view! {
                            <a href=resource.url class="resource-card">
                                <img src=resource.icon alt="Resource" class="resource-icon" />
                                <div class="resource-content">
                                    <h3>{resource.title}</h3>
                                    <p>{resource.description}</p>
                                </div>
                                <img src="/assets/icons/arrow-right.svg" alt="Go" class="resource-arrow" />
                            </a>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </section>
            
            // Help CTA
            <section class="help-cta">
                <div class="help-card">
                    <img src="/assets/icons/help.svg" alt="Help" class="help-icon" />
                    <div class="help-content">
                        <h3>"Need Help?"</h3>
                        <p>"Our community and support team are here to help you succeed"</p>
                    </div>
                    <div class="help-actions">
                        <a href="/community" class="btn-help">
                            "Join Discord"
                        </a>
                        <a href="/support" class="btn-help secondary">
                            "Contact Support"
                        </a>
                    </div>
                </div>
            </section>
            
            <Footer />
        </div>
    }
}
