export interface UploadTargetInfo {
  id: string;
  name: string;
  capabilities: {
    albums: boolean;
    sizeChoice: boolean;
  };
}

export type UploadTargetConfig =
  | { plugin: "local-folder"; root: string }
  | { plugin: "immich"; server_url: string; api_key: string }
  | {
      plugin: "s3";
      endpoint: string;
      bucket: string;
      region: string;
      access_key: string;
      secret_key: string;
      session_token: string | null;
      prefix: string;
    }
  | {
      plugin: "webdav";
      url: string;
      username: string;
      password: string;
      folder: string;
      allow_insecure_http: boolean;
    }
  | { plugin: "google-photos"; client_id: string; client_secret: string | null };

export type UploadSize =
  | { mode: "original" }
  | { mode: "resized"; max_edge: number };

export type UploadAlbumSelection =
  | { mode: "existing"; id: string; name: string }
  | { mode: "create"; name: string };

export interface UploadRequest {
  config: UploadTargetConfig;
  size: UploadSize;
  quality: number;
  album: UploadAlbumSelection | null;
  jobId: string | null;
}

export interface UploadAlbum {
  id: string;
  name: string;
}

export type UploadFileState =
  | "preparing"
  | "uploading"
  | "retrying"
  | "uploaded"
  | "already_present"
  | "failed"
  | "pending";

export interface UploadProgress {
  jobId: string;
  fileId: number;
  name: string;
  index: number;
  total: number;
  state: UploadFileState;
  message: string | null;
}

export interface UploadFileResult {
  fileId: number;
  name: string;
  state: UploadFileState;
  error: string | null;
}

export interface UploadBatchResult {
  jobId: string;
  files: UploadFileResult[];
  cancelled: boolean;
  paused: boolean;
}

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(command, args);
}

export function listUploadTargets(): Promise<UploadTargetInfo[]> {
  return call<UploadTargetInfo[]>("list_upload_targets");
}

export function listUploadAlbums(config: UploadTargetConfig): Promise<UploadAlbum[]> {
  return call<UploadAlbum[]>("list_upload_albums", { config });
}

export function googlePhotosSignedIn(clientId: string): Promise<boolean> {
  return call<boolean>("google_photos_signed_in", { clientId });
}

export function googlePhotosSignIn(clientId: string, clientSecret: string): Promise<void> {
  return call<void>("google_photos_sign_in", {
    clientId,
    clientSecret: clientSecret.trim() === "" ? null : clientSecret,
  });
}

export function googlePhotosSignOut(clientId: string): Promise<void> {
  return call<void>("google_photos_sign_out", { clientId });
}

export function uploadPhotos(ids: number[], request: UploadRequest): Promise<UploadBatchResult> {
  return call<UploadBatchResult>("upload_photos", { ids, request });
}

export function cancelUpload(jobId: string): Promise<void> {
  return call<void>("cancel_upload", { jobId });
}

export async function onUploadProgress(
  callback: (progress: UploadProgress) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<UploadProgress>("upload-progress", (event) => callback(event.payload));
}
