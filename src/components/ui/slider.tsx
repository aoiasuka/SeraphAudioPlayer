import * as React from "react";
import { cn } from "@/lib/utils";

interface SliderProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "type"> {
  value: number;
  min?: number;
  max?: number;
  step?: number;
}

export const Slider = React.forwardRef<HTMLInputElement, SliderProps>(
  ({ className, value, min = 0, max = 1, step = 0.01, ...props }, ref) => (
    <input
      ref={ref}
      type="range"
      value={value}
      min={min}
      max={max}
      step={step}
      className={cn(
        "h-1.5 bg-line appearance-none cursor-pointer accent-ink",
        className
      )}
      {...props}
    />
  )
);
Slider.displayName = "Slider";
