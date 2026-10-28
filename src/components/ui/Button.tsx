import { LoaderCircle, type LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "subtle";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "sm" | "md" | "lg";
  icon?: LucideIcon;
  iconRight?: LucideIcon;
  loading?: boolean;
  children?: ReactNode;
}

export function Button({ variant = "secondary", size = "md", icon: Icon, iconRight: IconRight, loading, children, className, disabled, type, ...rest }: ButtonProps) {
  const iconSize = size === "sm" ? 14 : size === "lg" ? 18 : 16;
  return (
    <button
      type={type ?? "button"}
      className={["btn", `btn-${variant}`, `btn-${size}`, className].filter(Boolean).join(" ")}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...rest}
    >
      {loading ? <LoaderCircle size={iconSize} className="spin" aria-hidden /> : Icon ? <Icon size={iconSize} aria-hidden /> : null}
      {children !== undefined && children !== null && <span className="btn-label">{children}</span>}
      {IconRight && !loading ? <IconRight size={iconSize} aria-hidden /> : null}
    </button>
  );
}

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: LucideIcon;
  label: string;
  variant?: ButtonVariant;
  size?: "sm" | "md";
  active?: boolean;
}

export function IconButton({ icon: Icon, label, variant = "ghost", size = "md", active, className, type, ...rest }: IconButtonProps) {
  return (
    <button
      type={type ?? "button"}
      className={["icon-btn", `btn-${variant}`, `icon-btn-${size}`, active ? "is-active" : null, className].filter(Boolean).join(" ")}
      aria-label={label}
      title={label}
      aria-pressed={active}
      {...rest}
    >
      <Icon size={size === "sm" ? 15 : 18} aria-hidden />
    </button>
  );
}
