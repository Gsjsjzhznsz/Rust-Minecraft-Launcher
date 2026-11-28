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
export type ContentEntry = { name: string; kind: string; sizeBytes: number; isDir: boolean; modifiedAt: number; disabled: boolean };
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

export type Page = "home" | "download" | "instances" | "account" | "settings";

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
  resolve: "解析依赖",
  download: "下载",
  deps: "依赖",
  "modpack-download": "整合包下载",
  "runtime-download": "运行时下载",
  "unpack-runtime": "解包",
  done: "完成",
  error: "出错",
};

/* ---- Modrinth 内容生态（与 Rust modrinth.rs 契约一致，snake_case） ---- */
export type SearchHit = {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  categories: string[];
  display_categories: string[];
  icon_url: string | null;
  downloads: number;
  author: string;
  date_modified: string;
};
export type SearchResp = { hits: SearchHit[]; total_hits: number };
export type VersionFile = {
  url: string;
  filename: string;
  primary: boolean;
  size: number;
  hashes: Record<string, string>;
};
export type VersionDependency = {
  version_id: string | null;
  project_id: string | null;
  file_name: string | null;
  dependency_type: string;
};
export type ProjectVersion = {
  id: string;
  name: string;
  version_number: string;
  changelog: string;
  game_versions: string[];
  loaders: string[];
  date_published: string;
  featured: boolean;
  downloads: number;
  files: VersionFile[];
  dependencies: VersionDependency[];
};
