import { focus } from "komorebi:host";
import { score, type WindowEvent } from "./scoring";

let invocation = 0;

export function invoke(event: WindowEvent): number {
  invocation += 1;
  const value = score(event, invocation);
  if (value % 3 === 0) {
    focus(value % 2 === 0 ? "left" : "right");
  }
  return value + invocation * 1000;
}

export function pureLoop(iterations: number): number {
  let accumulator = 1;
  for (let index = 0; index < iterations; index += 1) {
    accumulator = (accumulator * 48271 + index) % 2147483647;
  }
  return accumulator;
}

export function hostLoop(iterations: number): number {
  for (let index = 0; index < iterations; index += 1) {
    focus(index % 2 === 0 ? "left" : "right");
  }
  return iterations;
}

export function snapshot(): number {
  return invocation;
}

export function restore(value: number): void {
  invocation = value;
}
