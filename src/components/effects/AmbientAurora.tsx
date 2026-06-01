import { usePlayerStore } from "@/store/player";

export function AmbientAurora() {
  const track = usePlayerStore((s) => s.currentTrack());

  return (
    <>
      <div
        className="ambient-aurora -top-20 -left-10"
        style={{ backgroundColor: track?.glow1 ?? "#67e8f9" }}
      />
      <div
        className="ambient-aurora -bottom-20 -right-10"
        style={{ backgroundColor: track?.glow2 ?? "#a5b4fc" }}
      />
      <div
        className="ambient-aurora top-1/4 left-1/3 bg-blue-300"
        style={{ opacity: 0.12 }}
      />
    </>
  );
}
