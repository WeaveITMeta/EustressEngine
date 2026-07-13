-- Selection Counter — an example Studio plugin, authored in Luau, no Rust
-- recompile required. Drop this file (or a copy of it) into
-- %LOCALAPPDATA%/Eustress/Plugins/ and it appears on the Plugins tab next
-- to the built-in Road Builder section.
--
-- Exercises the full v1 plugin API: plugin:AddSection, plugin:AddButton,
-- plugin:Notify, plugin:GetSelection. There is no plugin:RegisterTab in v1
-- on purpose — there is only ever one "plugins" tab, already created by the
-- engine; a script adds a section/buttons to it, it doesn't create tabs.

plugin:AddSection("plugins", "selection-counter", "Selection Counter")

plugin:AddButton(
    "plugins",              -- tab id (always "plugins" in v1)
    "selection-counter",    -- section id, matching the AddSection call above
    "count-selection",      -- button id (unique within this section)
    "Count Selected",       -- label
    nil,                    -- icon (optional; nil = no icon)
    "Show how many entities are currently selected",  -- tooltip
    "selection_counter:count",  -- action id — must be globally unique across all plugins
    "normal",               -- size: "small" | "normal" | "large"
    function()
        local selected = plugin:GetSelection()
        local n = #selected
        if n == 0 then
            plugin:Notify("info", "Nothing selected.")
        elseif n == 1 then
            plugin:Notify("success", "1 entity selected: " .. selected[1])
        else
            plugin:Notify("success", n .. " entities selected.")
        end
    end
)
