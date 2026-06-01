import {
  Disc3,
  Heart,
  History,
  ListMusic,
  Music,
  Settings,
  Sliders,
  User,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useFluentHover } from "@/hooks/useFluentHover";
import { usePlayerStore } from "@/store/player";
import type { LibraryView } from "@/types/track";

interface NavItem {
  key: LibraryView | "settings";
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  onClickAction?: () => void;
}

export function Sidebar() {
  const activeView = usePlayerStore((s) => s.activeView);
  const setActiveView = usePlayerStore((s) => s.setActiveView);
  const toggleSettings = usePlayerStore((s) => s.toggleSettings);
  const onFluentMove = useFluentHover();

  const items: NavItem[] = [
    { key: "local", label: "本地音乐", icon: Music },
    { key: "recent", label: "最近播放", icon: History },
    { key: "liked", label: "我喜欢", icon: Heart },
    { key: "playlists", label: "歌单", icon: ListMusic },
    { key: "artists", label: "艺术家", icon: User },
    { key: "albums", label: "专辑", icon: Disc3 },
    { key: "settings", label: "设置", icon: Sliders, onClickAction: toggleSettings },
  ];

  return (
    <aside className="box-border w-[220px] min-w-[220px] max-w-[220px] flex-none flex flex-col justify-between p-4 acrylic-sidebar z-20 overflow-hidden">
      <div className="space-y-6">
        <nav className="space-y-1">
          {items.map((item) => {
            const Icon = item.icon;
            const active = item.key !== "settings" && item.key === activeView;
            return (
              <a
                key={item.key}
                href="#"
                onMouseMove={onFluentMove}
                onClick={(e) => {
                  e.preventDefault();
                  if (item.onClickAction) item.onClickAction();
                  else setActiveView(item.key as LibraryView);
                }}
                className={cn(
                  "fluent-item grid h-9 grid-cols-[16px_minmax(0,1fr)] items-center gap-3 px-3 text-xs font-medium rounded-lg transition-all",
                  active
                    ? "active-pill text-cyan-700 bg-black/[0.02]"
                    : "text-slate-600 hover:text-slate-900 hover:bg-black/[0.02]"
                )}
              >
                <Icon
                  className={cn(
                    "w-4 h-4 shrink-0",
                    active ? "text-cyan-600" : "text-slate-500"
                  )}
                />
                <span className="min-w-0 truncate">{item.label}</span>
              </a>
            );
          })}
        </nav>
      </div>

      <div className="pt-4 border-t border-black/[0.04] flex items-center justify-end">
        <button
          onClick={toggleSettings}
          className="w-7 h-7 flex items-center justify-center rounded-lg text-slate-500 hover:text-slate-800 hover:bg-black/[0.03] transition-colors"
          title="系统设置"
        >
          <Settings className="w-3 h-3" />
        </button>
      </div>
    </aside>
  );
}
