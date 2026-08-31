local scoring = {}

function scoring.score(event, invocation)
  return (event.window_id * 17 + event.workspace * 31 + invocation * 13) % 997
end

return scoring
