//! Writing a blueprint into the Space: the ScreenGui folders, the scripts
//! that run them, and the saved blueprint.
//!
//! - **GUI**: every [`Node`] becomes a folder with an `_instance.toml` in
//!   the GUI loader's own format ([`GuiTomlFile`]), written with
//!   `write_gui_toml` and spawned with `spawn_gui_element`, the same pair
//!   Insert > ScreenGui uses. Keys are the ones the loader reads:
//!   `position`/`size` as UDim2 4-tuples, `border_size`, `corner_radius`.
//! - **Scripts**: a module `<Name>UI` in ReplicatedStorage (opens and closes
//!   screens, sets HUD values, hands clicks to the game), a client script
//!   `<Name>UIStart` that starts it, and on request two hook scripts the
//!   user owns. All go through the `create_script` tool, so they have the
//!   SoulScript folder shape every other script has. The module and the
//!   start script are rewritten in place on every insert; hook scripts are
//!   written once and never touched again.
//! - **Saved blueprints**: `<Space>/.eustress/ui_builder/<Name>.json`, so a
//!   UI can be loaded back into the builder and changed.

use std::path::{Path, PathBuf};

use super::blueprint::{Blueprint, ElementKind, ScreenKind};
use super::layout::{Frame, Node, Role};
use crate::space::gui_loader::{GuiTomlFile, GuiTomlText};

/// Tag on every ScreenGui the builder inserts, and the per-UI tag that
/// finds them again for Update.
pub const TAG: &str = "UIBuilder";

pub fn ui_tag(name: &str) -> String {
    format!("{TAG}:{name}")
}

// ============================================================================
// GUI TOML
// ============================================================================

/// The `_instance.toml` for one node. `z` is its ZIndex.
pub fn gui_toml(node: &Node, display_name: &str, z: i32, blueprint_name: &str) -> GuiTomlFile {
    let mut def = crate::space::gui_loader::create_default_gui_toml(node.class, display_name);
    def.gui.position = eustress_common::ui_types::UDim2::new(node.position[0], node.position[1], node.position[2], node.position[3]);
    def.gui.size = eustress_common::ui_types::UDim2::new(node.size[0], node.size[1], node.size[2], node.size[3]);
    def.gui.anchor_point = node.anchor;
    def.gui.background_color = [node.bg[0], node.bg[1], node.bg[2], 1.0];
    def.gui.background_transparency = Some((1.0 - node.bg[3]).clamp(0.0, 1.0));
    def.gui.border_size = node.border;
    def.gui.border_color = [node.border_color[0], node.border_color[1], node.border_color[2], node.border_color[3]];
    def.gui.corner_radius = node.corner;
    def.gui.visible = node.visible;
    def.gui.z_index = z;

    if node.class == "ScreenGui" {
        def.gui.size = eustress_common::ui_types::UDim2::default();
        def.gui.background_transparency = Some(1.0);
        def.gui.enabled = Some(true);
        def.gui.reset_on_spawn = Some(false);
        def.tags = vec![TAG.to_string(), ui_tag(blueprint_name)];
    }

    if matches!(node.class, "TextLabel" | "TextButton" | "TextBox") {
        def.text = Some(GuiTomlText {
            text: node.text.clone(),
            text_color: [node.text_color[0], node.text_color[1], node.text_color[2], 1.0],
            text_transparency: (1.0 - node.text_color[3]).clamp(0.0, 1.0),
            text_stroke_color: [0.0, 0.0, 0.0, 1.0],
            text_stroke_transparency: 1.0,
            font_size: node.font_size,
            font: node.font.clone(),
            text_x_alignment: node.align.to_string(),
            text_y_alignment: "Center".to_string(),
            ..Default::default()
        });
    }
    def
}

// ============================================================================
// Scripts
// ============================================================================

/// Luau string literal with the characters that would end it escaped.
fn lua_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn lua_path(path: &[String]) -> String {
    format!("{{ {} }}", path.iter().map(|p| lua_str(p)).collect::<Vec<_>>().join(", "))
}

/// A number in a display value: "$1,250" is 1250, "12.4K" is 12400.
pub fn number_in(value: &str) -> Option<f64> {
    let v = value.trim();
    let mut digits = String::new();
    let mut seen_digit = false;
    for c in v.chars() {
        if c.is_ascii_digit() || (c == '.' && seen_digit) {
            seen_digit |= c.is_ascii_digit();
            digits.push(c);
        } else if c == ',' && seen_digit {
            continue;
        } else if seen_digit {
            break;
        }
    }
    let base: f64 = digits.parse().ok()?;
    let upper = v.to_ascii_uppercase();
    let mult = if upper.ends_with('K') { 1e3 } else if upper.ends_with('M') { 1e6 } else if upper.ends_with('B') { 1e9 } else { 1.0 };
    Some(base * mult)
}

/// What sits before the number in a display value: "$" in "$1,250".
fn prefix_of(value: &str) -> String {
    value.chars().take_while(|c| !c.is_ascii_digit()).collect::<String>().trim().to_string()
}

/// The module `<Name>UI`: everything the UI does in Play, generated from
/// the frames so every path it follows exists.
pub fn module_source(bp: &Blueprint, frames: &[Frame]) -> String {
    let name = &bp.name;
    let mut screens = String::new();
    let mut fullscreen = String::new();
    let mut menu = String::from("nil");
    for f in frames.iter().skip(1) {
        screens.push_str(&format!("\t[{}] = {},\n", lua_str(&f.id), lua_str(&f.gui_name(name))));
        if f.kind.is_some_and(|k| k.is_fullscreen()) {
            fullscreen.push_str(&format!("\t[{}] = true,\n", lua_str(&f.id)));
        }
        if f.kind == Some(ScreenKind::MainMenu) && menu == "nil" {
            menu = lua_str(&f.id);
        }
    }

    let mut values = String::new();
    let mut buttons = String::new();
    for f in frames {
        let gui = f.gui_name(name);
        for (i, n) in f.nodes.iter().enumerate() {
            match &n.role {
                Role::Value(id) => {
                    let el = bp.hud.iter().find(|e| &e.id == id);
                    let kind = el.map(|e| e.kind).unwrap_or(ElementKind::Counter);
                    let fill = if kind == ElementKind::Meter {
                        f.nodes.iter().enumerate()
                            .find(|(_, m)| m.role == Role::Fill(id.clone()))
                            .map(|(j, _)| lua_path(&f.path(j)))
                            .unwrap_or_else(|| "nil".to_string())
                    } else {
                        "nil".to_string()
                    };
                    let (digits, max, prefix) = el
                        .map(|e| (e.digits, e.max.clone(), prefix_of(&e.value)))
                        .unwrap_or_default();
                    values.push_str(&format!(
                        "\t[{}] = {{ gui = {}, path = {}, kind = {}, fill = {}, digits = {}, max = {}, prefix = {} }},\n",
                        lua_str(id), lua_str(&gui), lua_path(&f.path(i)), lua_str(kind.id()), fill, digits, lua_str(&max), lua_str(&prefix),
                    ));
                }
                Role::Open(to) => buttons.push_str(&format!("\t{{ gui = {}, path = {}, open = {} }},\n", lua_str(&gui), lua_path(&f.path(i)), lua_str(to))),
                Role::Close => buttons.push_str(&format!("\t{{ gui = {}, path = {}, close = {} }},\n", lua_str(&gui), lua_path(&f.path(i)), lua_str(&f.id))),
                Role::Action { action, item } => buttons.push_str(&format!(
                    "\t{{ gui = {}, path = {}, action = {}, item = {} }},\n",
                    lua_str(&gui), lua_path(&f.path(i)), lua_str(action), lua_str(item),
                )),
                _ => {}
            }
        }
    }

    let mut currencies = String::new();
    for e in bp.hud.iter().filter(|e| e.kind == ElementKind::Currency) {
        currencies.push_str(&format!("\t[{}] = {},\n", lua_str(&e.id), lua_str(&e.label)));
    }

    format!(
        r#"-- {name}UI: the game UI "{title}", made by the Eustress UI Builder.
-- The UI Builder rewrites this module every time the UI is inserted, so keep
-- your own code in {name}_UIHooks (or any script) and use it from there:
--
--   local UI = require(game:GetService("ReplicatedStorage"):WaitForChild("{name}UI"))
--   UI.set("hud_counter", 42)            -- a HUD value; a meter takes (value, max)
--   UI.open("shop")  UI.close("shop")    -- screens, by id
--   UI.on("buy", function(item) end)     -- a button the game handles
--   UI.find("codes", "Root", "Panel")    -- any part of a screen ("hud" for the HUD)

local Players = game:GetService("Players")

local UI = {{}}
local NAME = {name_lit}
local HUD = {hud_lit}
local MENU = {menu}

-- Screen id to ScreenGui name.
local SCREENS = {{
{screens}}}

-- Screens that cover the whole view; the HUD hides while one is open.
local FULLSCREEN = {{
{fullscreen}}}

-- HUD values: where each one's text (and a meter's fill) lives.
local VALUES = {{
{values}}}

-- Every button: it opens a screen, closes its own, or fires an action.
local BUTTONS = {{
{buttons}}}

-- Currency elements that follow a player attribute of the same name.
local CURRENCIES = {{
{currencies}}}

local handlers = {{}}
local started = false

local function playerGui()
	return Players.LocalPlayer:WaitForChild("PlayerGui")
end

local function walk(node, path)
	for _, name in ipairs(path) do
		if not node then
			return nil
		end
		node = node:FindFirstChild(name)
	end
	return node
end

local function guiOf(id)
	if id == "hud" then
		return playerGui():FindFirstChild(HUD)
	end
	local guiName = SCREENS[id]
	return guiName and playerGui():FindFirstChild(guiName)
end

--- Any part of a screen: UI.find("shop", "Root", "Panel", "Title").
function UI.find(id, ...)
	return walk(guiOf(id), {{ ... }})
end

local function setText(node, text)
	if not node then
		return
	end
	node.Text = text
	-- Plate-less kits draw a shade copy behind the text; keep it in step.
	local shade = node.Parent and node.Parent:FindFirstChild(node.Name .. "Shade")
	if shade then
		shade.Text = text
	end
end

local function withCommas(n)
	local s = tostring(math.floor(n + 0.5))
	local sign, digits = s:match("^(%-?)(%d+)$")
	if not digits then
		return s
	end
	local out = digits:reverse():gsub("(%d%d%d)", "%1,"):reverse()
	if out:sub(1, 1) == "," then
		out = out:sub(2)
	end
	return sign .. out
end

local function setHud(visible)
	local gui = guiOf("hud")
	if gui then
		gui.Enabled = visible
	end
end

function UI.open(id)
	if id == "hud" then
		setHud(true)
		return
	end
	local root = UI.find(id, "Root")
	if not root then
		warn(NAME .. "UI: there is no screen called " .. tostring(id))
		return
	end
	-- One pop-up screen at a time.
	if not FULLSCREEN[id] then
		for other in pairs(SCREENS) do
			if other ~= id and not FULLSCREEN[other] then
				local r = UI.find(other, "Root")
				if r then
					r.Visible = false
				end
			end
		end
	else
		setHud(false)
	end
	root.Visible = true
	if handlers.opened then
		handlers.opened(id)
	end
end

function UI.close(id)
	local root = UI.find(id, "Root")
	if root then
		root.Visible = false
	end
	if FULLSCREEN[id] then
		setHud(true)
	end
end

function UI.toggle(id)
	local root = UI.find(id, "Root")
	if root and root.Visible then
		UI.close(id)
	else
		UI.open(id)
	end
end

function UI.set(id, value, max)
	local v = VALUES[id]
	if not v then
		warn(NAME .. "UI: there is no HUD value called " .. tostring(id))
		return
	end
	local gui = playerGui():FindFirstChild(v.gui)
	local label = walk(gui, v.path)
	local n = tonumber(value)
	if v.kind == "meter" then
		local top = tonumber(max) or tonumber(v.max) or 100
		local f = math.clamp((n or 0) / top, 0, 1)
		local fill = v.fill and walk(gui, v.fill)
		if fill then
			fill.Size = UDim2.new(f, 0, 1, 0)
		end
		setText(label, tostring(math.floor(f * 100 + 0.5)) .. "%")
	elseif v.kind == "counter" then
		local text = tostring(value)
		if n and v.digits > 0 then
			text = string.format("%0" .. v.digits .. "d", math.floor(n))
		end
		local goal = max or (v.max ~= "" and v.max) or nil
		if goal then
			text = text .. "/" .. tostring(goal)
		end
		setText(label, text)
	elseif v.kind == "currency" and n then
		setText(label, v.prefix .. withCommas(n))
	else
		setText(label, tostring(value))
	end
end

function UI.on(action, fn)
	handlers[action] = fn
end

local function fire(action, item)
	local fn = handlers[action]
	if fn then
		fn(item)
	elseif action == "play" then
		-- The main menu's first button: back to the game.
		for id in pairs(FULLSCREEN) do
			UI.close(id)
		end
	else
		print(NAME .. "UI: nothing handles \"" .. action .. "\" (" .. tostring(item) .. ") yet. Add UI.on(\"" .. action .. "\", function(item) ... end).")
	end
end

--- Wire every button, follow the currencies, and show the loading screen
--- and then the main menu. Call once, from a client script.
function UI.start()
	if started then
		return
	end
	started = true
	local gui = playerGui()
	for _, b in ipairs(BUTTONS) do
		local node = walk(gui:WaitForChild(b.gui), b.path)
		if node then
			node.MouseButton1Click:Connect(function()
				if b.open then
					UI.open(b.open)
				elseif b.close then
					UI.close(b.close)
				else
					fire(b.action, b.item)
				end
			end)
		end
	end
	local player = Players.LocalPlayer
	for id, attribute in pairs(CURRENCIES) do
		local function show()
			local value = player:GetAttribute(attribute)
			if value ~= nil then
				UI.set(id, value)
			end
		end
		player:GetAttributeChangedSignal(attribute):Connect(show)
		show()
	end
	if SCREENS.loading then
		UI.open("loading")
		local fill = UI.find("loading", "Root", "Track", "Fill")
		for i = 1, 20 do
			if fill then
				fill.Size = UDim2.new(i / 20, 0, 1, 0)
			end
			task.wait(0.05)
		end
		UI.close("loading")
	end
	if MENU then
		UI.open(MENU)
	end
end

return UI
"#,
        name = name,
        title = bp.title.replace('"', "'"),
        name_lit = lua_str(name),
        hud_lit = lua_str(&format!("{name}_HUD")),
        menu = menu,
        screens = screens,
        fullscreen = fullscreen,
        values = values,
        buttons = buttons,
        currencies = currencies,
    )
}

/// The client script that starts the module.
pub fn start_source(bp: &Blueprint) -> String {
    format!(
        "-- Starts the {name} UI made by the Eustress UI Builder: wires its buttons,\n\
         -- shows the loading screen, then the main menu. Rewritten on every insert.\n\
         local UI = require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"{name}UI\"))\n\
         UI.start()\n",
        name = bp.name
    )
}

/// Every action the UI's buttons fire, with the items that fire it.
fn actions(frames: &[Frame]) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for f in frames {
        for n in &f.nodes {
            if let Role::Action { action, item } = &n.role {
                match out.iter_mut().find(|(a, _)| a == action) {
                    Some((_, items)) => {
                        if !item.is_empty() && !items.contains(item) {
                            items.push(item.clone());
                        }
                    }
                    None => out.push((action.clone(), if item.is_empty() { Vec::new() } else { vec![item.clone()] })),
                }
            }
        }
    }
    out
}

/// Actions the server decides: they spend or grant something.
fn server_action(action: &str) -> bool {
    matches!(action, "buy" | "claim" | "craft" | "redeem" | "buy_pass" | "offer" | "play_again")
}

/// The client hook script the user owns: a handler for every action.
pub fn client_hooks_source(bp: &Blueprint, frames: &[Frame]) -> String {
    let name = &bp.name;
    let mut body = String::new();
    for (action, items) in actions(frames) {
        let seen = if items.is_empty() { String::new() } else { format!(" -- {}", items.iter().take(6).cloned().collect::<Vec<_>>().join(", ")) };
        let line = match action.as_str() {
            "redeem" => format!(
                "UI.on(\"redeem\", function()\n\tlocal box = UI.find(\"codes\", \"Root\", \"Panel\", \"Body\", \"Code\")\n\tRequest:FireServer(\"redeem\", box and box.Text or \"\")\nend)\n"
            ),
            a if server_action(a) => format!("UI.on({}, function(item){seen}\n\tRequest:FireServer({}, item)\nend)\n", lua_str(a), lua_str(a)),
            "hotbar" => {
                let hotbar = bp.hud.iter().find(|e| e.kind == ElementKind::Hotbar).map(|e| e.id.clone()).unwrap_or_default();
                format!("UI.on(\"hotbar\", function(item){seen}\n\tUI.set({}, item)\nend)\n", lua_str(&hotbar))
            }
            a => format!("UI.on({}, function(item){seen}\n\tprint(\"{name}: {a}\", item)\nend)\n", lua_str(a)),
        };
        body.push_str(&line);
        body.push('\n');
    }
    format!(
        "-- Your code for the {name} UI. The UI Builder wrote this once and never\n\
         -- overwrites it: change anything. The module it uses, {name}UI, is\n\
         -- regenerated on every insert.\n\
         local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\n\
         local UI = require(ReplicatedStorage:WaitForChild(\"{name}UI\"))\n\
         local Request = ReplicatedStorage:WaitForChild(\"{name}Remotes\"):WaitForChild(\"Request\")\n\
         \n\
         -- Set HUD values from your game, for example:\n\
         --   UI.set(\"{first}\", 10)\n\
         \n\
         {body}"
        ,
        name = name,
        first = bp.hud.iter().find(|e| !matches!(e.kind, ElementKind::Button | ElementKind::Menu | ElementKind::Controls | ElementKind::Minimap)).map(|e| e.id.as_str()).unwrap_or("hud_value"),
        body = body,
    )
}

/// The server hook script the user owns: currencies as player attributes,
/// and the requests the UI's buttons send.
pub fn server_hooks_source(bp: &Blueprint) -> String {
    let name = &bp.name;
    let mut currencies = String::new();
    for e in bp.hud.iter().filter(|e| e.kind == ElementKind::Currency) {
        let start = number_in(&e.value).unwrap_or(0.0).round() as i64;
        currencies.push_str(&format!("\t[{}] = {},\n", lua_str(&e.label), start));
    }
    let main = bp.hud.iter().find(|e| e.kind == ElementKind::Currency).map(|e| e.label.clone()).unwrap_or_else(|| "Coins".to_string());
    let mut prices = String::new();
    let mut robux = String::new();
    for s in bp.screens.iter().filter(|s| s.kind == ScreenKind::Shop) {
        for i in &s.items {
            if super::generate::is_robux_price(&i.value) {
                robux.push_str(&format!("\t[{}] = 0, -- {}\n", lua_str(&i.name), i.value));
            } else if let Some(n) = number_in(&i.value) {
                prices.push_str(&format!("\t[{}] = {},\n", lua_str(&i.name), n.round() as i64));
            }
        }
    }
    format!(
        r#"-- Server side of the {name} UI: its currencies as player attributes, and
-- the requests its buttons send. The UI Builder wrote this once and never
-- overwrites it: change anything.
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local remotes = ReplicatedStorage:FindFirstChild("{name}Remotes") or Instance.new("Folder")
remotes.Name = "{name}Remotes"
remotes.Parent = ReplicatedStorage
local Request = remotes:FindFirstChild("Request") or Instance.new("RemoteEvent")
Request.Name = "Request"
Request.Parent = remotes

-- Starting balances: the HUD's values when the UI was made. The HUD shows
-- each one live, because the UI follows the attribute of the same name.
local CURRENCIES = {{
{currencies}}}
local MAIN = {main}

-- In-game prices, in {main_plain}.
local PRICES = {{
{prices}}}

-- Robux products: put each product id here and prompt it with
-- MarketplaceService:PromptProductPurchase.
local PRODUCTS = {{
{robux}}}

local function setup(player)
	for currency, start in pairs(CURRENCIES) do
		if player:GetAttribute(currency) == nil then
			player:SetAttribute(currency, start)
		end
	end
end
Players.PlayerAdded:Connect(setup)
for _, player in ipairs(Players:GetPlayers()) do
	setup(player)
end

Request.OnServerEvent:Connect(function(player, action, item)
	if action == "buy" then
		local price = PRICES[item]
		if price == nil then
			if PRODUCTS[item] ~= nil then
				print("{name}: " .. tostring(item) .. " is a Robux product; give it an id in PRODUCTS")
			end
			return
		end
		local balance = player:GetAttribute(MAIN) or 0
		if balance < price then
			return
		end
		player:SetAttribute(MAIN, balance - price)
		player:SetAttribute("Owns" .. tostring(item):gsub("%W", ""), true)
	elseif action == "claim" or action == "craft" then
		print("{name}: " .. player.Name .. " wants to " .. action .. " " .. tostring(item))
	elseif action == "redeem" then
		print("{name}: " .. player.Name .. " entered the code " .. tostring(item))
	else
		print("{name}: " .. player.Name .. " asked for " .. tostring(action) .. " " .. tostring(item))
	end
end)
"#,
        name = name,
        currencies = currencies,
        main = lua_str(&main),
        main_plain = main,
        prices = prices,
        robux = robux,
    )
}

// ============================================================================
// Saved blueprints
// ============================================================================

pub fn saved_dir(space_root: &Path) -> PathBuf {
    space_root.join(".eustress").join("ui_builder")
}

pub fn saved_path(space_root: &Path, name: &str) -> PathBuf {
    saved_dir(space_root).join(format!("{name}.json"))
}

pub fn save(space_root: &Path, bp: &Blueprint) -> Result<PathBuf, String> {
    let dir = saved_dir(space_root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = saved_path(space_root, &bp.name);
    let json = serde_json::to_string_pretty(bp).map_err(|e| e.to_string())?;
    crate::space::gui_loader::write_atomic(&path, json.as_bytes()).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

pub fn load(path: &Path) -> Result<Blueprint, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every saved blueprint in the Space, newest first.
pub fn list_saved(space_root: &Path) -> Vec<(PathBuf, Blueprint, std::time::SystemTime)> {
    let Ok(entries) = std::fs::read_dir(saved_dir(space_root)) else { return Vec::new() };
    let mut out: Vec<(PathBuf, Blueprint, std::time::SystemTime)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| {
            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            load(&p).ok().map(|bp| (p, bp, modified))
        })
        .collect();
    out.sort_by(|a, b| b.2.cmp(&a.2));
    out
}

/// Move a saved blueprint to the Space's trash; nothing is deleted.
pub fn trash_saved(space_root: &Path, name: &str) -> Result<PathBuf, String> {
    let from = saved_path(space_root, name);
    let to = crate::undo::Action::reserve_trash_path(space_root, &from);
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&from, &to).map_err(|e| format!("move {} to the trash: {e}", from.display()))?;
    Ok(to)
}

/// "4 min ago", for the Recent and Saved lists.
pub fn ago(t: std::time::SystemTime) -> String {
    let secs = std::time::SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_builder::generate::{generate, seed_for};
    use crate::ui_builder::layout::frames;

    #[test]
    fn numbers_in_display_values() {
        assert_eq!(number_in("$1,250"), Some(1250.0));
        assert_eq!(number_in("12.4K"), Some(12400.0));
        assert_eq!(number_in("R$ 199"), Some(199.0));
        assert_eq!(number_in("Free"), None);
        assert_eq!(prefix_of("$1,250"), "$");
    }

    #[test]
    fn the_module_names_every_screen_and_escapes_quotes() {
        let mut bp = generate("a fps shooter", seed_for("a fps shooter"));
        bp.title = "The \"Best\" Game".to_string();
        let fr = frames(&bp);
        let src = module_source(&bp, &fr);
        for f in fr.iter().skip(1) {
            assert!(src.contains(&format!("\"{}\"", f.gui_name(&bp.name))), "{} missing", f.id);
        }
        assert!(src.contains("MouseButton1Click"));
        assert!(!src.contains("\"Best\""), "a raw quote would end the comment's string");
        let hooks = client_hooks_source(&bp, &fr);
        assert!(hooks.contains("UI.on(\"buy\""));
        let server = server_hooks_source(&bp);
        assert!(server.contains("OnServerEvent"));
    }

    #[test]
    fn a_node_becomes_loader_toml() {
        let bp = generate("a tycoon", 1);
        let fr = frames(&bp);
        let hud = &fr[0];
        let def = gui_toml(&hud.nodes[1], &hud.nodes[1].name, 11, &bp.name);
        let text = toml::to_string_pretty(&def).unwrap();
        let back = crate::space::gui_loader::load_gui_definition_from_str(&text).unwrap();
        assert_eq!(back.gui.position, def.gui.position);
        assert_eq!(back.gui.z_index, 11);
        let root = gui_toml(&hud.nodes[0], "X_HUD", 10, &bp.name);
        assert!(root.tags.iter().any(|t| t == &ui_tag(&bp.name)));
    }
}
