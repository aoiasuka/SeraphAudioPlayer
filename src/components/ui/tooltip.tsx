import * as React from "react";
import { cn } from "@/lib/utils";

interface TooltipProps {
  label: string;
  children: React.ReactNode;
  side?: "top" | "bottom";
  className?: string;
}

/**
 * 极简 tooltip：用 title 属性 + 自定义视觉版本。
 * 这里只走 native title 实现，避免引入 Radix；
 * 后续真要悬浮卡片再升级。
 */
export function Tooltip({ label, children, className }: TooltipProps) {
  return (
    <span title={label} className={cn("inline-flex", className)}>
      {children}
    </span>
  );
}
