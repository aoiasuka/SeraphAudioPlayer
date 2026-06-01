export interface LyricLine {
  time: number;
  text: string;
}

export interface Track {
  id: string;
  title: string;
  artist: string;
  album: string;
  albumYear?: string;
  cover: string;
  format: string;
  bitdepth: string;
  sampleRate?: string;
  bitrate: string;
  channels: string;
  size: string;
  path: string;
  sourceUrl?: string | null;
  sourceId?: string | null;
  cacheMissing?: boolean;
  duration: number;
  glowColor: string;
  glow1?: string;
  glow2?: string;
  lyrics: LyricLine[];
}

export interface OutputDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export type DriverKind = "wasapi" | "asio" | "direct";

export type LibraryView =
  | "local"
  | "streaming"
  | "recent"
  | "liked"
  | "playlists"
  | "artists"
  | "albums";
