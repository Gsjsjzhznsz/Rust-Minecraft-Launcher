/* Quartz UI —— 与 Rust 后端契约一致的类型定义 */

export type Account = { kind: string; username: string; uuid: string };
export type AccountEntry = Account & { selected: boolean };
export type VersionInfo = { id: string; type: string; url: string };
export type DownloadProgress = {
  stage: string;
  current: number;
  total: number;
  percent: number;
  file: string;
  done: boolean;
  error?: string;
};
export type EnvInfo = {
  gameDir: string;
  storageMode: string;
  layoutOk: boolean;
  layoutError: string | null;
  fullAccess: boolean;
  freeBytes?: number | null;
};
export type AppMeta = { name: string; version: string; os: string; arch: string };
export type Preflight = {
  versionInstalled: boolean;
  versionJsonOk: boolean;
  javaMajor: number;
  accountOk: boolean;
  runtimeInstalled: boolean;
  runtimeSpecAvailable: boolean;
  runtimeVersion: string | null;
  runtimeDir: string;
};
export type LaunchLog = { source: string; line: string };
export type LaunchExit = { code: number; version: string; logTail?: string | null };
export type MigrateMsg = { percent: number; done: boolean; error: string | null };

export type RuntimeStatus = { installed: boolean; version: string | null; path: string; specAvailable: boolean };
export type RendererInfo = {
  id: string;
  name: string;
  note: string;
  eglLib: string;
  glLib: string;
  sizeHint: string;
  specAvailable: boolean;
  installed: boolean;
  active: boolean;
};
export type LoaderEntry = { version: string; stable: boolean };
export type InstanceEntry = {
  id: string;
  name: string;
  mcVersion: string;
  loaderDesc: string;
  javaMajor: number;
  memoryMb: number;
  renderer: string | null;
  createdAt: string;
  selected: boolean;
};
export type ContentEntry = { name: string; kind: string; sizeBytes: number; isDir: boolean; modifiedAt: number };
export type SettingsInfo = {
  extraJvmArgs: string[];
  azureClientIdSet: boolean;
  azureClientId: string | null;
  selectedInstance: string | null;
};
export type AssetsStatus = {
  indexPresent: boolean;
  indexId: string;
  total: number;
  missing: number;
  missingBytes: number;
};
export type LogEntry = { name: string; size: number; modifiedMs: number };
export type Diagnostics = { lastCrash: string | null; lastLaunchTail: string | null; freeBytes: number | null };
export type LwjglStatus = { installed: boolean; path: string };
export type MsPrompt = { userCode: string; verificationUri: string; message?: string | null };
export type MsResult = { ok: boolean; error?: string | null; account?: Account | null };

export type Page = "home" | "download" | "instances" | "content" | "account" | "settings";

export const KIND_LABELS: Record<string, string> = {
  release: "正式版",
  snapshot: "快照",
  old_beta: "Beta",
  old_alpha: "Alpha",
};
export const KIND_ORDER = ["release", "snapshot", "old_beta", "old_alpha"] as const;
export const PAGE_SIZE = 50;

export const STAGE_LABELS: Record<string, string> = {
  manifest: "版本清单",
  "version-json": "版本 JSON",
  client: "客户端 jar",
  libraries: "依赖库",
  "asset-index": "资源索引",
  assets: "资源文件",
  done: "完成",
  error: "出错",
};
