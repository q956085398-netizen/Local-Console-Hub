export interface LaunchCandidate {
  path: string;
  command: string;
  cwd: string;
  port: number | null;
}
export interface DirectoryScan {
  name: string;
  cwd: string;
  candidates: LaunchCandidate[];
  logs: string[];
  warnings: string[];
}
export type PathKind = "directory" | "program" | "log";

export function isDirectoryScan(value: unknown): value is DirectoryScan {
  if (typeof value !== "object" || value === null) return false;
  const scan = value as Record<string, unknown>;
  return (
    typeof scan.name === "string" &&
    typeof scan.cwd === "string" &&
    Array.isArray(scan.logs) &&
    scan.logs.every((item) => typeof item === "string") &&
    Array.isArray(scan.warnings) &&
    scan.warnings.every((item) => typeof item === "string") &&
    Array.isArray(scan.candidates) &&
    scan.candidates.every((item: unknown) => {
      if (typeof item !== "object" || item === null) return false;
      const candidate = item as Record<string, unknown>;
      return (
        typeof candidate.path === "string" &&
        typeof candidate.command === "string" &&
        typeof candidate.cwd === "string" &&
        (candidate.port === null ||
          (typeof candidate.port === "number" &&
            Number.isInteger(candidate.port) &&
            candidate.port > 0 &&
            candidate.port <= 65535))
      );
    })
  );
}

export function selectedProgram(path: string): LaunchCandidate {
  const split = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  const command = path.toLowerCase().endsWith(".ps1")
    ? `powershell.exe -NoProfile -File "${path}"`
    : `"${path}"`;
  const end = split === 2 && path[1] === ":" ? split + 1 : split;
  return { path, command, cwd: path.slice(0, end), port: null };
}

export function applicationUrl(protocol: string, address: string): string | undefined {
  const trimmed = address.trim();
  if (!trimmed) return undefined;
  return /^https?:\/\//i.test(trimmed) ? trimmed : `${protocol}${trimmed}`;
}
