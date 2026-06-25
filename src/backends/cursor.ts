import { spawn as nodeSpawn } from "node:child_process";
import type { ChildProcess, SpawnOptions } from "node:child_process";
import { EventEmitter } from "node:events";
import type { JobSpec, RawCursorJson } from "../types.js";
import { initStreamState, parseLine } from "../stream.js";
import type { Backend, BackendHandle, BackendResult } from "./types.js";

export type SpawnFn = (
  command: string,
  args: string[],
  options: SpawnOptions,
) => ChildProcess;

/**
 * The cursor-agent backend adapter. Spawns the child, parses stream-json (NDJSON) lines,
 * emits "progress"/"stderr" events, and resolves a BackendResult on close.
 */
export function makeCursorAdapter(deps?: { spawnFn?: SpawnFn }): Backend {
  const spawnFn = deps?.spawnFn ?? (nodeSpawn as SpawnFn);

  return {
    run(spec: JobSpec): BackendHandle {
      const events = new EventEmitter();
      const child = spawnFn(spec.bin, spec.argv, {
        cwd: spec.cwd,
        stdio: ["ignore", "pipe", "pipe"],
      });

      const state = initStreamState();
      let stdoutBuf = "";
      let stderrBuf = "";
      let resultRaw: RawCursorJson | null = null;

      const emitProgress = () => {
        events.emit("progress", {
          lastTool: state.lastTool,
          tokensSoFar: state.tokensSoFar,
          lastAssistant: state.lastAssistant,
          filesTouched: [...state.filesTouched],
          phase: state.phase,
        });
      };

      const handleLine = (line: string) => {
        const parsed = parseLine(line, state);
        if (parsed.result) resultRaw = parsed.result;
        if (parsed.changed) emitProgress();
      };

      child.stdout?.on("data", (chunk: Buffer) => {
        stdoutBuf += chunk.toString();
        let idx: number;
        while ((idx = stdoutBuf.indexOf("\n")) !== -1) {
          const line = stdoutBuf.slice(0, idx);
          stdoutBuf = stdoutBuf.slice(idx + 1);
          handleLine(line);
        }
      });

      child.stderr?.on("data", (chunk: Buffer) => {
        const s = chunk.toString();
        stderrBuf += s;
        events.emit("stderr", s);
      });

      const done = new Promise<BackendResult>((resolve) => {
        let settled = false;
        const finish = (r: BackendResult) => {
          if (settled) return;
          settled = true;
          resolve(r);
        };

        child.on("close", (code) => {
          // Flush a trailing line that arrived without a terminating newline.
          if (stdoutBuf.length > 0) {
            handleLine(stdoutBuf);
            stdoutBuf = "";
          }
          finish({
            raw: resultRaw ?? {},
            cleanExit: code === 0,
            stderr: stderrBuf,
          });
        });

        child.on("error", (err: Error) => {
          finish({
            raw: { is_error: true, result: String(err?.message ?? err) },
            cleanExit: false,
            stderr: stderrBuf + String(err?.message ?? err),
          });
        });
      });

      return { child, events, done };
    },
  };
}
