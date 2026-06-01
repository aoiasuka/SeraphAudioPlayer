import type { Track } from "@/types/track";

export const mockPlaylist: Track[] = [];

export const mockDevices = [
  { id: "wasapi:hd-dac1", name: "WASAPI: HD-DAC1", isDefault: true },
  { id: "asio:xmos", name: "ASIO: XMOS Driver", isDefault: false },
  { id: "directsound:speaker", name: "DirectSound: Speaker", isDefault: false },
];
