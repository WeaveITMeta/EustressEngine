// =============================================================================
// Eustress Web - Blog Index Page
// =============================================================================
// Lists all blog posts. Each post is a long-form essay or sales letter targeting
// a specific audience (indie studios, scripters, sim researchers, etc.).
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[derive(Clone, Debug, PartialEq)]
struct BlogPost {
    slug: &'static str,
    title: &'static str,
    excerpt: &'static str,
    audience: &'static str,
    read_minutes: u32,
}

fn get_posts() -> Vec<BlogPost> {
    vec![
        BlogPost {
            slug: "indie-studios",
            title: "The Roblox Model Is a Trap — And Every Studio That Doesn't See It Yet Is Already Inside the Cage",
            excerpt: "A new simulation engine just went open source. It runs a full year of world simulation in one second, renders at AAA quality, and takes zero percent of your revenue. Here's why that matters more than any feature list.",
            audience: "Indie Studios",
            read_minutes: 12,
        },
    ]
}

#[component]
pub fn BlogPage() -> impl IntoView {
    let posts = get_posts();

    view! {
        <div class="page page-press">
            <CentralNav active="".to_string() />

            <div class="press-bg">
                <div class="press-grid-overlay"></div>
            </div>

            <div class="press-container">
                <div class="press-header">
                    <div class="hero-header">
                        <div class="header-line"></div>
                        <span class="header-tag">"BLOG"</span>
                        <div class="header-line"></div>
                    </div>
                    <h1 class="press-title">"Eustress Blog"</h1>
                    <p class="press-subtitle">
                        "Essays, sales letters, and deep dives on simulation, ownership, "
                        "and the future of indie game development."
                    </p>
                </div>

                <section class="press-section">
                    <h2>"Latest"</h2>
                    <div class="blog-posts-grid">
                        {posts.into_iter().map(|post| {
                            let href = format!("/blog/{}", post.slug);
                            view! {
                                <a href=href class="blog-post-card">
                                    <div class="blog-post-meta">
                                        <span class="blog-post-audience">{post.audience}</span>
                                        <span class="blog-post-read-time">
                                            {format!("{} min read", post.read_minutes)}
                                        </span>
                                    </div>
                                    <h3 class="blog-post-title">{post.title}</h3>
                                    <p class="blog-post-excerpt">{post.excerpt}</p>
                                    <span class="blog-post-cta">"Read post →"</span>
                                </a>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                </section>
            </div>

            <Footer />
        </div>
    }
}
