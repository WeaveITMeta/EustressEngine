# Sim Page README and Tabs

**Status:** built 2026-11-14, staging only. Founder direction (McKale, 2026-11-14): "I want the description to be either plain text + About as an .md file. posted from Eustress Engine's Website explorer directory. The README.md in there will be the about section. That way pages can have .md format for improving visuals, including links, setting a readme like GitHub appearance to their simulations. A short form description will be where the current one is, and the .md format optional will be centered and below it with the tabs, and their contents below as selected. The format will be like this, tabs across the bottom of the short form description. Options for Servers (live status) Shop (Eustress in sim purchases), Links, and About. Details moves from the left aligned bottom to the right section below download Eustress Player."

Builds on `SIM_PAGE_THEME_AND_LINKS.md` (PageSpec v1, the link review, the page images). Everything here follows that document's rule: nothing an author writes reaches the public page until it is reviewed, and a pending or removed piece is simply not shown.

## 1. What a creator does

Put a `README.md` file in the Space's **Website** folder (the Website service in the Explorer). It is optional. At publish, Studio uploads it with the themed page, and the listing page shows it, rendered, under an **About** tab.

- **Size:** at most **32 KiB** (32,768 bytes) of UTF-8. A bigger file is not uploaded, and the publish log says why.
- **Links:** ordinary Markdown links, `<https://...>` autolinks and bare `https://` addresses. Each one goes through the off-site link review (URL Scanner, the AI link judge, risk score at most 25). A link that is not approved is shown as its text with no link; a bare address shows `[link pending review]` or `[link removed]`.
- **Images:** `![alt](path)` where `path` is a file **inside the Space**, resolved from the Website folder the way GitHub resolves a README (`Assets/diagram.png`, `../Assets/diagram.png`). PNG, JPEG or WebP, at most 2 MB each, at most **8** images. Each is uploaded to its own reviewed slot (`readme-0` to `readme-7`) and judged under the whole Guardian Code like the gallery. A remote image (`https://...`, `data:`) is never loaded: it is replaced by its alt text, so no tracking pixel reaches a visitor.
- **Raw HTML** in the file is never rendered. It is dropped.

## 2. Data contract

### Studio to the Worker (at publish, after the page images and the PageSpec)

| Request | Body | When |
|---|---|---|
| `PUT /api/simulations/{id}/page/image/readme-N` | the image bytes, as the other page images | each README image, before the README |
| `PUT /api/simulations/{id}/page/readme` | `Content-Type: text/markdown; charset=utf-8`, the README text with each image target rewritten to `eustress-image:readme-N` | the Space has a README |
| `DELETE /api/simulations/{id}/page/readme` | none | the Space has no README (removes a previously published one) |

Studio's bake (`engine/src/website/page.rs`) rewrites image targets and refuses an oversize file; the Worker checks everything again and trusts nothing Studio sends.

### The Worker's public view

`GET /api/simulations/{id}/page` gains `readme`, and answers 200 with `theme: null` and no links when a listing has a README but no PageSpec:

```json
"readme": {
  "markdown": "# Battery lab\n\nSee [our paper](https://example.edu/paper) ...\n\n![Cell](/api/simulations/{id}/page/image/readme-0)",
  "links": ["https://example.edu/paper"],
  "images": ["/api/simulations/{id}/page/image/readme-0"]
}
```

- `readme` is `null` unless the README text itself is approved (section 3) and the stored text is the text that was reviewed.
- `markdown` is the reviewed text with every link target the review has not approved for this listing removed, every bare URL masked, and every image target that is not an approved `readme-N` slot replaced by its alt text.
- `links` is every link target left in `markdown`; `images` is every image source left in it. The website renders a link or an image **only** if it is in these lists, so a parsing difference between the Worker and the website can never produce an unreviewed link.
- A child-directed listing gets no links (section 7 of the theme document).

### Website rendering

`pulldown-cmark` events are turned into Leptos elements from a fixed set: paragraphs, headings (`#` is drawn as a second-level heading, so the page keeps one title), emphasis, strong, strikethrough, inline code, code blocks, lists, block quotes, tables, rules, line breaks, links and images. The page never sets `inner_html`. HTML events are dropped. A link must be `https://` and in `readme.links`; it opens in a new tab with `rel="nofollow noopener noreferrer ugc"` and its domain beside it. An image must be in `readme.images`.

## 3. The README review

The README is reviewed as text, on its own, the way page images are reviewed as pictures:

1. **The text.** One AI call with the whole Guardian Code as the system prompt and the README inside a per-request nonce frame as untrusted data. Approved only when no article applies. A malformed answer is asked once more; a second failure removes it (`review_failed`, fail closed). No API key or no policy text: it stays pending.
2. **Its links.** Every link target becomes a link review entry with source `readme`, reviewed exactly like a description link (cache, scan, judge, re-review every 7 days).
3. **Its images.** Each `readme-N` slot is a page image and is judged as one.

The README also ships inside the published package, where the listing judge reads `.md` files, so the Code applies to it twice: once with the listing, once on its own for the page.

A removal is told to the author in the same moderation feed as removed links and images.

## 4. The listing page layout

Left column (`g-lcopy`): the gallery, then the short plain-text description (no heading), then a tab bar, then the selected tab:

| Tab | Shows | When there is nothing |
|---|---|---|
| **Servers** | live status: the author's hosted session, each server, Join | the Offline line |
| **Shop** | the Space's products (`PassStore`, `GET /api/commerce/catalog/{id}`) | the store's own empty state |
| **Links** | the approved page links, grouped | tab hidden |
| **About** | the rendered README, centered, GitHub README width | tab hidden |

The first tab shown is About when there is one, else Servers. Tabs are buttons with `role="tab"`, arrow keys move between them, and the bar scrolls sideways on its own at phone width, never the page.

Right column (`g-lside`): **Details** moves here, below "Download Eustress Player" (and "Open a copy in Studio" for an open-source listing).

## 5. Where each part lives

| Part | Files |
|---|---|
| Studio bake and upload | `eustress/crates/engine/src/website/page.rs`, `eustress/crates/common/src/page_spec.rs` (README rules), `eustress/crates/engine/src/ui/file_event_handler.rs` (upload) |
| Studio discoverability | `eustress/crates/common/assets/service_properties/Website.toml`, the `website_*` MCP tools |
| Worker | `infrastructure/cloudflare/api/src/page.mjs` (routes, public view), `src/link_review.mjs` (README links, images, text review), tests in `tests/page.test.mjs`, `tests/link_review.test.mjs` |
| Website | `eustress/crates/web/src/api/sim_page.rs` (`Readme`), `src/components/readme.rs` (the renderer), `src/pages/listing.rs` (tabs, Details), `style/sim_page.css` |
