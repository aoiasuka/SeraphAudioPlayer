import {
  Activity,
  Disc3,
  Heart,
  History,
  ListMusic,
  Music,
  Radio,
  Sliders,
  SlidersHorizontal,
  User,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";
import type { LibraryView } from "@/types/track";

interface NavItem {
  key: LibraryView | "settings";
  label: string;
  icon: React.ComponentType<{ className?: string }>;
}

interface NavGroup {
  tab: string;
  items: NavItem[];
}

// 发现18：groups 是纯静态数据，移到模块级避免每次渲染重建
const NAV_GROUPS: NavGroup[] = [
  {
    tab: "DRAWER A — 资料库",
    items: [
      { key: "local", label: "本地音乐", icon: Music },
      { key: "streaming", label: "流媒体", icon: Radio },
      { key: "recent", label: "最近播放", icon: History },
      { key: "liked", label: "我喜欢", icon: Heart },
    ],
  },
  {
    tab: "DRAWER B — 浏览",
    items: [
      { key: "playlists", label: "歌单", icon: ListMusic },
      { key: "artists", label: "艺术家", icon: User },
      { key: "albums", label: "专辑", icon: Disc3 },
    ],
  },
  {
    tab: "DRAWER C — 系统",
    items: [
      { key: "eq", label: "EQ 均衡器", icon: SlidersHorizontal },
      { key: "analysis", label: "声学分析", icon: Activity },
      { key: "settings", label: "设置", icon: Sliders },
    ],
  },
];

export function Sidebar() {
  const activeView = usePlayerStore((s) => s.activeView);
  const setActiveView = usePlayerStore((s) => s.setActiveView);
  const toggleSettings = usePlayerStore((s) => s.toggleSettings);
  const loginStatus = usePlayerStore((s) => s.bilibiliLoginStatus);

  return (
    <aside className="box-border w-[clamp(180px,18vw,228px)] min-w-[180px] max-w-[228px] flex-none flex flex-col border-r-2 border-ink bg-paper pt-6 pb-5 z-20 overflow-hidden">
      <div className="px-6 pb-5 border-b-2 border-ink">
        <h2 className="font-tw text-2xl font-bold tracking-tight text-ink">
          SERAPH<span className="text-stamp">_</span>
        </h2>
        <p className="font-tw text-[10px] text-ink2 mt-0.5 tracking-widest">
          AUDIO ARCHIVE SYSTEM v2.0
        </p>
      </div>

      <nav className="flex flex-col px-3.5 mt-4 overflow-y-auto no-scrollbar flex-1">
        {NAV_GROUPS.map((group) => (
          <div key={group.tab}>
            <div className="font-tw text-[9px] tracking-[3px] text-ink3 px-3 pt-4 pb-1.5">
              {group.tab}
            </div>
            {group.items.map((item) => {
              const Icon = item.icon;
              const active = item.key !== "settings" && item.key === activeView;
              return (
                <button
                  key={item.key}
                  type="button"
                  onClick={() => {
                    if (item.key === "settings") toggleSettings();
                    else setActiveView(item.key);
                  }}
                  className={cn(
                    "group flex w-full items-center gap-2.5 px-3 py-2 font-tw text-[13px] text-left border-[1.5px] transition-all",
                    active
                      ? "text-ink bg-card border-ink font-bold shadow-[2.5px_2.5px_0_var(--ink)]"
                      : "text-ink2 border-transparent hover:text-ink hover:bg-card"
                  )}
                >
                  <span
                    className={cn(
                      "text-[11px] leading-none",
                      active ? "text-stamp" : "text-ink3"
                    )}
                  >
                    {active ? "■" : "□"}
                  </span>
                  <Icon className="w-3.5 h-3.5 shrink-0" />
                  <span className="min-w-0 truncate">{item.label}</span>
                </button>
              );
            })}
          </div>
        ))}
      </nav>

      {loginStatus.loggedIn && loginStatus.username && (
        <div className="mt-auto mx-6 pt-4 border-t border-dashed border-line">
          <div className="font-tw text-[11px] leading-[1.9] text-ink2">
            OPERATOR
            <br />
            <b className="text-ink text-[13px]">{loginStatus.username}</b>
            <br />
            SERAPH ARCHIVE <span className="text-brown">● ONLINE</span>
          </div>
        </div>
      )}
    </aside>
  );
}
