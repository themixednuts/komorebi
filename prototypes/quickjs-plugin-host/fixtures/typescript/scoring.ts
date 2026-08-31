export interface WindowEvent {
  windowId: number;
  workspace: number;
}

export function score(event: WindowEvent, invocation: number): number {
  return (event.windowId * 17 + event.workspace * 31 + invocation * 13) % 997;
}
