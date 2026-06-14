import { useState, useCallback, useRef, useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { PinOff } from "lucide-react";
import type { Provider } from "@/types";
import type { SessionRouteEntry } from "@/types/proxy";
import { proxyApi } from "@/lib/api/proxy";
import { cn } from "@/lib/utils";

interface SessionPillProps {
  session: SessionRouteEntry;
  appId: string;
  providers: Provider[];
}

/** 格式化相对时间 */
function relativeTime(timestamp: number): string {
  const diff = Date.now() - timestamp;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}小时前`;
  return `${Math.floor(diff / 86_400_000)}天前`;
}

/** 根据字符串生成稳定的颜色索引 */
function stableColorIndex(str: string): number {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    hash = ((hash << 5) - hash + str.charCodeAt(i)) | 0;
  }
  return Math.abs(hash) % SESSION_COLORS.length;
}

// 可区分的色板（避免过于鲜艳或过于暗淡）
const SESSION_COLORS = [
  { bg: "bg-blue-500/15", border: "border-blue-500/40", text: "text-blue-700 dark:text-blue-300", dot: "bg-blue-500" },
  { bg: "bg-violet-500/15", border: "border-violet-500/40", text: "text-violet-700 dark:text-violet-300", dot: "bg-violet-500" },
  { bg: "bg-amber-500/15", border: "border-amber-500/40", text: "text-amber-700 dark:text-amber-300", dot: "bg-amber-500" },
  { bg: "bg-rose-500/15", border: "border-rose-500/40", text: "text-rose-700 dark:text-rose-300", dot: "bg-rose-500" },
  { bg: "bg-cyan-500/15", border: "border-cyan-500/40", text: "text-cyan-700 dark:text-cyan-300", dot: "bg-cyan-500" },
  { bg: "bg-emerald-500/15", border: "border-emerald-500/40", text: "text-emerald-700 dark:text-emerald-300", dot: "bg-emerald-500" },
];

/** session pill：点击弹出菜单，可切换 provider 或解除 pin */
export function SessionPill({ session, appId, providers }: SessionPillProps) {
  const pillRef = useRef<HTMLSpanElement>(null);
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const queryClient = useQueryClient();

  const color = SESSION_COLORS[stableColorIndex(session.sessionId)];
  const label =
    session.displayName || session.sessionId.slice(0, 8) || "session";
  const shortId = session.sessionId.slice(0, 8);
  // 项目目录名（仅在与 display name 不同时额外显示）
  const projectBasename = session.projectDir
    ? session.projectDir.split("/").pop() || null
    : null;
  const showProject = projectBasename && projectBasename !== label;

  // 点击 pill 切换菜单
  const handleToggle = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      setOpen((prev) => !prev);
    },
    [],
  );

  // 点击菜单外部关闭
  useEffect(() => {
    if (!open) return;
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && menuRef.current.contains(e.target as Node)) {
        return;
      }
      if (pillRef.current && pillRef.current.contains(e.target as Node)) {
        return;
      }
      setOpen(false);
    };
    const handleScroll = () => setOpen(false);
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("scroll", handleScroll, true);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("scroll", handleScroll, true);
    };
  }, [open]);

  const handleSwitchSession = useCallback(
    async (targetProviderId: string) => {
      try {
        await proxyApi.setSessionRoute(appId, session.sessionId, targetProviderId);
        setOpen(false);
        await queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
        const target = providers.find((p) => p.id === targetProviderId);
        toast.success(
          `Session ${label} 已切换到 ${target?.name || targetProviderId}`,
        );
      } catch (e) {
        toast.error(`切换 session 失败: ${e}`);
      }
    },
    [appId, session, providers, queryClient, label],
  );

  const handleUnpin = useCallback(async () => {
    try {
      await proxyApi.removeSessionRoute(appId, session.sessionId);
      setOpen(false);
      await queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      toast.success(`Session ${label} 已恢复跟随全局`);
    } catch (e) {
      toast.error(`解除 pin 失败: ${e}`);
    }
  }, [appId, session, queryClient, label]);

  // 计算菜单定位：在 pill 下方左对齐
  const menuStyle = (() => {
    if (!open || !pillRef.current) return undefined;
    const rect = pillRef.current.getBoundingClientRect();
    return { left: rect.left, top: rect.bottom + 4 };
  })();

  return (
    <>
      {/* pill 标签 */}
      <span
        ref={pillRef}
        className={cn(
          "inline-flex items-center gap-1 rounded-md px-1.5 py-0.5",
          "text-[10px] font-medium border cursor-pointer select-none",
          "transition-colors hover:brightness-110",
          session.isRouted
            ? `${color.bg} ${color.border} ${color.text}`
            : `${color.bg} ${color.border} ${color.text} opacity-60`,
        )}
        title={`点击管理 session · ID: ${shortId}${session.projectDir ? ` · ${session.projectDir}` : ""}`}
        onClick={handleToggle}
      >
        <span className={cn("inline-block w-1.5 h-1.5 rounded-full", color.dot)} />
        {label}
        {showProject && (
          <span className="opacity-60 ml-0.5">
            {projectBasename}
          </span>
        )}
      </span>

      {/* 下拉菜单（fixed 定位在 pill 下方） */}
      {menuStyle && (
        <div
          ref={menuRef}
          className="fixed z-50 min-w-[200px] rounded-lg border border-border bg-popover p-1 shadow-md animate-in fade-in-0 zoom-in-95"
          style={menuStyle}
        >
          {/* session 信息头 */}
          <div className="px-2 py-1.5 border-b border-border mb-1">
            <p className="text-xs font-semibold">{label}</p>
            <div className="flex flex-wrap gap-x-3 gap-y-0.5 mt-0.5 text-[10px] text-muted-foreground">
              <span>ID: {shortId}</span>
              {session.projectDir && (
                <span
                  title={session.projectDir}
                  className="cursor-pointer hover:text-primary hover:underline"
                  onClick={(e) => {
                    e.stopPropagation();
                    proxyApi.openPathInExplorer(session.projectDir!);
                  }}
                >
                  📁 {session.projectDir.split("/").pop()}
                </span>
              )}
              <span>⏱ {relativeTime(session.lastActiveAt)}</span>
            </div>
          </div>

          {/* provider 切换列表 */}
          <p className="text-[10px] text-muted-foreground px-2 pb-1">切换供应商：</p>
          {providers.map((p) => (
            <button
              key={p.id}
              className={cn(
                "w-full text-left px-2 py-1 rounded text-xs transition-colors",
                "hover:bg-muted/80",
                p.id === session.providerId &&
                  "bg-primary/10 text-primary font-medium",
              )}
              disabled={p.id === session.providerId}
              onClick={(e) => {
                e.stopPropagation();
                handleSwitchSession(p.id);
              }}
            >
              {p.name}
              {p.id === session.providerId && " ✓"}
            </button>
          ))}

          {/* 解除 pin */}
          {session.isRouted && (
            <>
              <div className="h-px bg-border mx-1 my-1" />
              <button
                className="w-full text-left px-2 py-1 rounded text-xs text-orange-600 dark:text-orange-400 hover:bg-orange-500/10 transition-colors flex items-center gap-1.5"
                onClick={(e) => {
                  e.stopPropagation();
                  handleUnpin();
                }}
              >
                <PinOff className="w-3 h-3" />
                跟随全局默认
              </button>
            </>
          )}
        </div>
      )}
    </>
  );
}
