import { ChevronRight } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import {
  isSeparator,
  useContextMenuStore,
  type ContextMenuAction,
} from "@/store/contextMenu";
import { CreatePlaylistWithTracksDialog } from "./CreatePlaylistWithTracksDialog";
import { DeleteTrackConfirmDialog } from "./DeleteTrackConfirmDialog";
import { TrackInfoDialog } from "./TrackInfoDialog";

/** 菜单面板通用样式：沿用 DeviceMenu 确立的纸质档案弹层视觉。 */
const MENU_PANEL_CLASS =
  "bg-card border-2 border-ink shadow-[4px_4px_0_rgba(43,39,34,0.18)] p-1.5";

function MenuSeparatorLine() {
  return <div className="mx-1 my-1 border-t border-dashed border-line" />;
}

function MenuRow({
  entry,
  onClose,
  submenuSide,
}: {
  entry: ContextMenuAction;
  onClose: () => void;
  submenuSide: "left" | "right";
}) {
  const [submenuOpen, setSubmenuOpen] = useState(false);
  const [submenuAlign, setSubmenuAlign] = useState<"top" | "bottom">("top");
  const rowRef = useRef<HTMLDivElement | null>(null);
  const Icon = entry.icon;
  const hasChildren = !!entry.children && entry.children.length > 0;

  return (
    <div
      ref={rowRef}
      className="relative"
      onPointerEnter={() => {
        if (!hasChildren || entry.disabled) return;
        // 行靠近视口下缘时子菜单向上对齐，避免被窗口裁掉
        const rect = rowRef.current?.getBoundingClientRect();
        setSubmenuAlign(
          rect && rect.top > window.innerHeight * 0.55 ? "bottom" : "top"
        );
        setSubmenuOpen(true);
      }}
      onPointerLeave={() => setSubmenuOpen(false)}
    >
      <button
        type="button"
        disabled={entry.disabled}
        onClick={() => {
          if (entry.disabled || hasChildren) return;
          entry.onSelect?.();
          onClose();
        }}
        className={cn(
          "flex w-full items-center gap-2 px-2 py-1.5 text-left font-tw text-[11px] font-bold transition-colors",
          entry.danger
            ? "text-stamp hover:bg-stamp/10"
            : "text-ink2 hover:bg-paper2 hover:text-ink",
          entry.disabled &&
            "cursor-not-allowed opacity-40 hover:bg-transparent hover:text-ink2"
        )}
      >
        {Icon ? <Icon className="h-3.5 w-3.5 shrink-0" /> : null}
        <span className="min-w-0 flex-1 truncate">{entry.label}</span>
        {entry.hint ? (
          <span className="shrink-0 text-[9px] font-normal text-ink3">
            {entry.hint}
          </span>
        ) : null}
        {hasChildren ? (
          <ChevronRight className="h-3 w-3 shrink-0 text-ink3" />
        ) : null}
      </button>
      {/* 子菜单紧贴父面板（无间隙），跨越时 pointerleave 不会误触发收起 */}
      {submenuOpen && hasChildren ? (
        <div
          className={cn(
            MENU_PANEL_CLASS,
            "absolute z-10 max-h-[55vh] w-48 space-y-0.5 overflow-y-auto",
            submenuSide === "right" ? "left-full" : "right-full",
            submenuAlign === "top" ? "top-0" : "bottom-0"
          )}
        >
          {entry.children?.map((child) =>
            isSeparator(child) ? (
              <MenuSeparatorLine key={child.key} />
            ) : (
              <MenuRow
                key={child.key}
                entry={child}
                onClose={onClose}
                submenuSide={submenuSide}
              />
            )
          )}
        </div>
      ) : null}
    </div>
  );
}

/**
 * 全局右键菜单层：
 * - 屏蔽 WebView2 默认右键菜单（文本输入区豁免，保留系统复制/粘贴）
 * - 渲染唯一的自绘菜单实例（视口边界自动翻转、外点/Esc/滚动/失焦关闭）
 * - 承载曲目信息 / 新建歌单并加入 / 删除确认三个全局弹窗
 */
export function ContextMenuLayer() {
  const open = useContextMenuStore((s) => s.open);
  const x = useContextMenuStore((s) => s.x);
  const y = useContextMenuStore((s) => s.y);
  const entries = useContextMenuStore((s) => s.entries);
  const closeContextMenu = useContextMenuStore((s) => s.closeContextMenu);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [pos, setPos] = useState({ left: 0, top: 0 });
  const [measured, setMeasured] = useState(false);
  const [submenuSide, setSubmenuSide] = useState<"left" | "right">("right");

  // 全局屏蔽默认右键菜单；input/textarea/可编辑区保留系统编辑菜单
  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest("input, textarea")) return;
      if (target instanceof HTMLElement && target.isContentEditable) return;
      event.preventDefault();
    };
    window.addEventListener("contextmenu", onContextMenu);
    return () => window.removeEventListener("contextmenu", onContextMenu);
  }, []);

  // 打开期间：菜单外按下 / Esc / 列表滚动 / 窗口变化 / 失焦 → 关闭
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) closeContextMenu();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeContextMenu();
    };
    const onScroll = (event: Event) => {
      // 子菜单内部滚动（歌单较多时）不关闭菜单
      if (
        event.target instanceof Node &&
        rootRef.current?.contains(event.target)
      ) {
        return;
      }
      closeContextMenu();
    };
    const onWindowChange = () => closeContextMenu();
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onWindowChange);
    window.addEventListener("blur", onWindowChange);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onWindowChange);
      window.removeEventListener("blur", onWindowChange);
    };
  }, [open, closeContextMenu]);

  // 视口边界翻转：先隐形渲染在触发坐标，测量实际尺寸后按需向左/上翻
  useLayoutEffect(() => {
    if (!open) {
      setMeasured(false);
      return;
    }
    const element = rootRef.current;
    if (!element) return;
    const rect = element.getBoundingClientRect();
    const overflowRight = x + rect.width > window.innerWidth - 8;
    const overflowBottom = y + rect.height > window.innerHeight - 8;
    const left = overflowRight ? Math.max(8, x - rect.width) : x;
    setPos({
      left,
      top: overflowBottom ? Math.max(8, y - rect.height) : y,
    });
    // 一级菜单右侧放不下二级面板（约 192px）时，子菜单向左展开
    setSubmenuSide(
      left + rect.width + 200 > window.innerWidth ? "left" : "right"
    );
    setMeasured(true);
  }, [open, x, y, entries]);

  return (
    <>
      {open ? (
        <div
          ref={rootRef}
          className={cn(
            MENU_PANEL_CLASS,
            "ctx-menu-panel fixed z-[90] w-52 space-y-0.5 select-none",
            !measured && "invisible"
          )}
          style={{ left: measured ? pos.left : x, top: measured ? pos.top : y }}
        >
          {entries.map((entry) =>
            isSeparator(entry) ? (
              <MenuSeparatorLine key={entry.key} />
            ) : (
              <MenuRow
                key={entry.key}
                entry={entry}
                onClose={closeContextMenu}
                submenuSide={submenuSide}
              />
            )
          )}
        </div>
      ) : null}
      <TrackInfoDialog />
      <CreatePlaylistWithTracksDialog />
      <DeleteTrackConfirmDialog />
    </>
  );
}
