---@meta

--- Mutable Lua-side request builder that becomes immutable at invocation.
---@class (exact) PluginActionBuilder
local PluginActionBuilder = {}

---@param parameter string
---@param value PluginValue
PluginActionBuilder["set"] = function(self, parameter, value) end

---@param parameter string
---@param values PluginValue[]
PluginActionBuilder["set_list"] = function(self, parameter, values) end

--- Capability-checked object passed to every extension lifecycle callback.
---@class (exact) PluginContext
---@field plugin_id string (readonly)
local PluginContext = {}

---@param action string
---@return PluginActionBuilder
PluginContext["action"] = function(self, action) end

---@param value boolean
---@return PluginValue
PluginContext["boolean"] = function(self, value) end

---@param value string
---@return PluginValue
PluginContext["choice"] = function(self, value) end

---@param red integer
---@param green integer
---@param blue integer
---@param alpha integer
---@return PluginValue
PluginContext["color"] = function(self, red, green, blue, alpha) end

---@param message string
PluginContext["debug"] = function(self, message) end

---@param value string
---@return PluginValue
PluginContext["decimal"] = function(self, value) end

---@param kind string
---@param id string
---@return PluginValue
PluginContext["entity"] = function(self, kind, id) end

---@param message string
PluginContext["error"] = function(self, message) end

---@param message string
PluginContext["info"] = function(self, message) end

---@param action PluginActionBuilder
PluginContext["invoke"] = function(self, action) end

---@param value string
---@return PluginValue
PluginContext["selector"] = function(self, value) end

---@param value integer
---@return PluginValue
PluginContext["signed"] = function(self, value) end

---@param value string
---@return PluginValue
PluginContext["text"] = function(self, value) end

---@param message string
PluginContext["trace"] = function(self, message) end

---@param unit string
---@param magnitude integer
---@return PluginValue
PluginContext["unit"] = function(self, unit, magnitude) end

---@param value integer
---@return PluginValue
PluginContext["unsigned"] = function(self, value) end

---@param message string
PluginContext["warn"] = function(self, message) end

---@param units integer[]
---@return PluginValue
PluginContext["windows_path"] = function(self, units) end

--- Opaque, typed protocol scalar used to construct an action request in Lua.
---@class (exact) PluginValue
local PluginValue = {}
