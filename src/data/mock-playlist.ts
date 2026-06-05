import type { Track } from "@/types/track";

export const mockPlaylist: Track[] = [];

export const mockDevices = [
  { id: "wasapi:hd-dac1", name: "WASAPI: HD-DAC1", isDefault: true },
  { id: "directsound:speaker", name: "System Shared: Speaker", isDefault: false },
];
