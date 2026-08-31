local scoring = require("scoring")
local invocation = 0

local plugin = {}

function plugin.invoke(event)
  invocation = invocation + 1
  local value = scoring.score(event, invocation)
  if value % 3 == 0 then
    focus(value % 2 == 0 and "left" or "right")
  end
  return value + invocation * 1000
end

function plugin.pure_loop(iterations)
  local accumulator = 1
  for index = 0, iterations - 1 do
    accumulator = (accumulator * 48271 + index) % 2147483647
  end
  return accumulator
end

function plugin.host_loop(iterations)
  for index = 0, iterations - 1 do
    focus(index % 2 == 0 and "left" or "right")
  end
  return iterations
end

function plugin.snapshot()
  return invocation
end

function plugin.restore(value)
  invocation = value
end

return plugin
