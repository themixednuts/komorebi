---@meta

--- Capability-checked object passed to every extension lifecycle callback.
---@class (exact) PluginContext
---@field plugin_id string (readonly)
local PluginContext = {}

---@param message string
PluginContext["debug"] = function(self, message) end

---@param message string
PluginContext["error"] = function(self, message) end

---@param message string
PluginContext["info"] = function(self, message) end

---@param message string
PluginContext["trace"] = function(self, message) end

---@param message string
PluginContext["warn"] = function(self, message) end
