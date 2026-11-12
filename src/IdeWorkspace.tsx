import Editor, { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  Activity,
  Check,
  Cloud,
  ChevronDown,
  ChevronRight,
  Code2,
  Eye,
  FileCode2,
  FilePlus2,
  Files,
  Folder,
  FolderOpen,
  FolderPlus,
  LoaderCircle,
  HardDrive,
  ListTodo,
  MessageSquareText,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  RefreshCw,
  Search,
  Send,
  Sparkles,
  ShieldAlert,
  ShieldCheck,
  X,
} from "lucide-react";
import { type CSSProperties, type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import editorWorker from "../node_modules/monaco-editor/esm/vs/editor/editor.worker.js?worker";
import jsonWorker from "../node_modules/monaco-editor/esm/vs/language/json/json.worker.js?worker";
import cssWorker from "../node_modules/monaco-editor/esm/vs/language/css/css.worker.js?worker";
import htmlWorker from "../node_modules/monaco-editor/esm/vs/language/html/html.worker.js?worker";
import tsWorker from "../node_modules/monaco-editor/esm/vs/language/typescript/ts.worker.js?worker";
import "./ide.css";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { AiAnalysisToolbar, AiInsightsPanel, useIdeInsights } from "./ideIntelligence";
import { estimateUsageCost, formatUsd, type PricingMap } from "./modelPricing";

const monacoGlobal = globalThis as typeof globalThis & {
  MonacoEnvironment?: { getWorker: (_moduleId: string, label: string) => Worker };
};
monacoGlobal.MonacoEnvironment = {
  getWorker(_moduleId, label) {
    if (label === "json") return new jsonWorker();
    if (["css", "scss", "less"].includes(label)) return new cssWorker();
    if (["html", "handlebars", "razor"].includes(label)) return new htmlWorker();
    if (["typescript", "javascript"].includes(label)) return new tsWorker();
    return new editorWorker();
  },
};
loader.config({ monaco });
monaco.editor.defineTheme("jadx-engineering-principles", { base:"vs-dark",inherit:true,rules:[{token:"comment",foreground:"85858f"},{token:"string",foreground:"c7ff9f"}],colors:{"editor.background":"#141415","editor.foreground":"#eeeeee","editor.selectionBackground":"#c7ff9f25","editor.lineHighlightBackground":"#ffffff04","editorLineNumber.foreground":"#77777f"} });

type Workspace = { id: string; name: string; root_path: string; archived: boolean; exists: boolean };
type WorkspaceEntry = { path: string; kind: string; bytes: number; modified_at: string };
type WorkspacePage = { entries: WorkspaceEntry[]; next_cursor?: string | null; scanned: number };
type IdeFile = { path: string; content: string; etag: string; bytes: number };
type SearchResult = { path: string; line?: number | null; preview?: string | null };
type SearchResponse = { results: SearchResult[]; scannedFiles: number; bounded: boolean };
type IdeTab = { path: string; content: string; savedContent: string | null; etag: string | null; previewOnly?: boolean; showPreview?: boolean };
type TreeRow = { kind: "entry"; entry: WorkspaceEntry; depth: number } | { kind: "more"; path: string; depth: number };
type CodeOssStatus = { installed: boolean; running: boolean; url: string | null; version: string; archiveBytes: number };
type ExecutionMode = "Safe" | "ApproveForMe" | "DeveloperUnrestricted";
type AgentMessage = { id: string; role: string; content: string; streaming?: boolean };
type AgentVisualPreview = { name: string; mime_type: string; data_url: string; size_bytes: number };
type AgentFileChange = { path: string; mode: string; added_lines: number; removed_lines: number; bytes_before?: number; bytes_after?: number; recovery_snapshot?: string; diff_preview?: string };
type SmartSelection = { path: string; text: string; truncated: boolean; startLine: number; endLine: number; left: number; top: number; width: number };
type CloudFallback = { prompt: string; reason: string; freshContext?: boolean; maxToolIterations?: number; planMode?: boolean; providers: { id: string; name: string; model: string; maxContext: number }[] };

const IMAGE_PREVIEW_EXTENSIONS = new Set(["avif", "bmp", "gif", "ico", "jpeg", "jpg", "png", "webp"]);
const BINARY_PREVIEW_EXTENSIONS = new Set(["pdf", ...IMAGE_PREVIEW_EXTENSIONS]);
const HTML_PREVIEW_EXTENSIONS = new Set(["html", "htm", "svg"]);
const HTML_PREVIEW_CSP = "default-src 'none'; img-src data: blob: workspace-preview:; font-src data: workspace-preview:; media-src data: blob: workspace-preview:; style-src 'unsafe-inline' data: workspace-preview:; script-src 'unsafe-inline' 'unsafe-eval' workspace-preview:; connect-src workspace-preview:; frame-src data: blob: workspace-preview:; object-src data: workspace-preview:; form-action 'none'; base-uri workspace-preview:";

function fileExtension(path: string) {
  return path.split("/").pop()?.split(".").pop()?.toLowerCase() ?? "";
}

function isBinaryPreviewPath(path: string) {
  return BINARY_PREVIEW_EXTENSIONS.has(fileExtension(path));
}

function isPreviewablePath(path: string) {
  return BINARY_PREVIEW_EXTENSIONS.has(fileExtension(path)) || HTML_PREVIEW_EXTENSIONS.has(fileExtension(path));
}

function workspacePreviewUrl(token: string, path: string) {
  const encodedPath = path.split("/").map(encodeURIComponent).join("/");
  return `workspace-preview://localhost/${encodeURIComponent(token)}/${encodedPath}`;
}

function workspacePreviewDirectoryUrl(token: string, path: string) {
  const directory = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
  const encodedPath = directory ? `${directory.split("/").map(encodeURIComponent).join("/")}/` : "";
  return `workspace-preview://localhost/${encodeURIComponent(token)}/${encodedPath}`;
}

function htmlPreviewDocument(source: string, baseUrl: string) {
  const injectedHead = `<meta http-equiv="Content-Security-Policy" content="${HTML_PREVIEW_CSP}"><base href="${baseUrl}">`;
  if (/<head\b[^>]*>/i.test(source)) {
    return source.replace(/<head\b[^>]*>/i, (head) => `${head}${injectedHead}`);
  }
  if (/<html\b[^>]*>/i.test(source)) {
    return source.replace(/<html\b[^>]*>/i, (html) => `${html}<head>${injectedHead}</head>`);
  }
  return `<!doctype html><html><head>${injectedHead}</head><body>${source}</body></html>`;
}

const PERMISSION_DESCRIPTIONS: Record<ExecutionMode, string> = {
  Safe: "Ask before workspace edits, commands, and external actions.",
  ApproveForMe: "Apply workspace edits automatically. Allowlisted IDE checks run in the sandbox; other commands still ask.",
  DeveloperUnrestricted: "Workspace edits and shell/Python run without per-action approval, as your macOS user. Never elevates privileges.",
};

type Props = {
  workspaces: Workspace[];
  activeWorkspaceId: string | null;
  activeConversationId: string | null;
  workspaceChats: { id: string; title: string }[];
  onNewWorkspaceChat: () => void | Promise<void>;
  onSelectWorkspaceChat: (id: string) => void | Promise<void>;
  providerName: string;
  providerId: string;
  modelName: string;
  contextLength: number;
  contextMaximum: number;
  usagePricing: PricingMap;
  supportsMaxReasoning: boolean;
  cloudFallback: CloudFallback | null;
  onApproveCloudFallback: (providerId: string) => void;
  onDismissCloudFallback: () => void;
  onOpenProviderSettings: () => void;
  executionMode: ExecutionMode;
  onExecutionMode: (value: ExecutionMode) => void;
  agentToolsEnabled: boolean;
  agentToolLimitation?: string | null;
  isActive: boolean;
  engineeringPrinciplesMode: boolean;
  onInspectPrinciples: () => void;
  onSelectWorkspace: (id: string) => void | Promise<void>;
  onAddWorkspace: () => void | Promise<void>;
  onOpenWorkspacePath: (path: string) => void | Promise<void>;
  onRunAgent: (prompt: string, options?: { freshContext?: boolean; maxToolIterations?: number; planMode?: boolean }) => void;
  onStopAgent: () => void;
  agentStarting: boolean;
  agentRunning: boolean;
  agentStatus: string;
  agentMessages: AgentMessage[];
  agentActivity: string[];
  agentPlan: ReactNode;
  securityPrompts: ReactNode;
  agentTodo: ReactNode;
  agentError: string | null;
  onDismissAgentError: () => void;
  agentFileChanges: AgentFileChange[];
  agentVisualPreview: AgentVisualPreview | null;
  onDismissAgentVisualPreview: () => void;
  onUndoAgentChanges: (changes: AgentFileChange[]) => void;
  onReviewAgentChanges: (changes: AgentFileChange[]) => void;
  agentRefreshVersion: number;
};

export default function IdeWorkspace({
  workspaces,
  activeWorkspaceId,
  activeConversationId,
  workspaceChats,
  onNewWorkspaceChat,
  onSelectWorkspaceChat,
  providerName,
  providerId,
  modelName,
  contextLength,
  contextMaximum,
  usagePricing,
  supportsMaxReasoning,
  cloudFallback,
  onApproveCloudFallback,
  onDismissCloudFallback,
  onOpenProviderSettings,
  executionMode,
  onExecutionMode,
  agentToolsEnabled,
  agentToolLimitation,
  isActive,
  engineeringPrinciplesMode,
  onInspectPrinciples,
  onSelectWorkspace,
  onAddWorkspace,
  onOpenWorkspacePath,
  onRunAgent,
  onStopAgent,
  agentStarting,
  agentRunning,
  agentMessages,
  agentActivity,
  agentPlan,
  securityPrompts,
  agentTodo,
  agentError,
  onDismissAgentError,
  agentFileChanges,
  agentVisualPreview,
  onDismissAgentVisualPreview,
  onUndoAgentChanges,
  onReviewAgentChanges,
  agentRefreshVersion,
}: Props) {
  const workspace = workspaces.find((item) => item.id === activeWorkspaceId);
  const [directories, setDirectories] = useState<Record<string, WorkspacePage>>({});
  const [loadingDirectories, setLoadingDirectories] = useState<Set<string>>(() => new Set());
  const [expandedDirectories, setExpandedDirectories] = useState<Set<string>>(() => new Set(["."]));
  const [tabsByWorkspace, setTabsByWorkspace] = useState<Record<string, IdeTab[]>>({});
  const [activePathsByWorkspace, setActivePathsByWorkspace] = useState<Record<string, string>>({});
  const [selectedFolder, setSelectedFolder] = useState(".");
  const [pane, setPane] = useState<"files" | "search">("files");
  const [searchQuery, setSearchQuery] = useState("");
  const [submittedSearch, setSubmittedSearch] = useState("");
  const [searchRun, setSearchRun] = useState(0);
  const [searchText, setSearchText] = useState(false);
  const [searchResults, setSearchResults] = useState<SearchResult[]>([]);
  const [searchStatus, setSearchStatus] = useState("");
  const [searching, setSearching] = useState(false);
  const [searchingError, setSearchingError] = useState("");
  const [showAgentPanel, setShowAgentPanel] = useState(true);
  const [agentPanelView, setAgentPanelView] = useState<"agent"|"insights">("agent");
  const [ossContext, setOssContext] = useState<{path?:string;selectionStart?:number;selectionEnd?:number;selectionText:string;dirty:boolean;connected:boolean}>({selectionText:"",dirty:false,connected:false});
  const [showExplorer, setShowExplorer] = useState(true);
  const [explorerWidth, setExplorerWidth] = useState(258);
  const [resizingExplorer, setResizingExplorer] = useState(false);
  const [agentPanelWidth, setAgentPanelWidth] = useState(360);
  const [resizingAgentPanel, setResizingAgentPanel] = useState(false);
  const [agentPrompt, setAgentPrompt] = useState("");
  const [planMode, setPlanMode] = useState(false);
  const [includeSelection, setIncludeSelection] = useState(true);
  const [selectedCode, setSelectedCode] = useState("");
  const [smartMode, setSmartMode] = useState(false);
  const [smartSelection, setSmartSelection] = useState<SmartSelection | null>(null);
  const [smartPrompt, setSmartPrompt] = useState("");
  const [editorError, setEditorError] = useState("");
  const [savingPaths, setSavingPaths] = useState<Set<string>>(() => new Set());
  const [treeScrollTop, setTreeScrollTop] = useState(0);
  const [treeViewportHeight, setTreeViewportHeight] = useState(440);
  const [jumpTo, setJumpTo] = useState<{ path: string; line: number } | null>(null);
  const [folderDragActive, setFolderDragActive] = useState(false);
  const [codeOssStatus, setCodeOssStatus] = useState<CodeOssStatus | null>(null);
  const [codeOssUrl, setCodeOssUrl] = useState<string | null>(null);
  const [codeOssOpen, setCodeOssOpen] = useState(false);
  const [codeOssWorkspaceId, setCodeOssWorkspaceId] = useState<string | null>(null);
  const [codeOssBusy, setCodeOssBusy] = useState(false);
  const [codeOssProgress, setCodeOssProgress] = useState<{ downloaded: number; total: number } | null>(null);
  const [codeOssError, setCodeOssError] = useState("");
  const [previewToken, setPreviewToken] = useState<string | null>(null);
  const [previewSource, setPreviewSource] = useState<{ workspaceId: string; path: string; content: string } | null>(null);
  const [previewReloadKey, setPreviewReloadKey] = useState(0);
  const onOpenWorkspacePathRef = useRef(onOpenWorkspacePath);
  const treeViewportRef = useRef<HTMLDivElement>(null);
  const workspaceViewRef = useRef<HTMLElement>(null);
  const editorCanvasRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const smartModeRef = useRef(false);
  const smartSelectionRef = useRef<SmartSelection | null>(null);
  const agentTranscriptRef = useRef<HTMLDivElement>(null);
  const saveActiveRef = useRef<() => void>(() => undefined);
  const workspaceKey = workspace?.id ?? "";
  const workspaceKeyRef = useRef(workspaceKey);
  workspaceKeyRef.current = workspaceKey;
  onOpenWorkspacePathRef.current = onOpenWorkspacePath;
  const refreshAgentFilesRef = useRef<() => void>(() => undefined);
  const tabs = tabsByWorkspace[workspaceKey] ?? [];
  const activePath = activePathsByWorkspace[workspaceKey] ?? "";
  const activePathRef = useRef(activePath);
  activePathRef.current = activePath;
  const activeTab = tabs.find((tab) => tab.path === activePath);
  const dirtyCount = tabs.filter((tab) => tab.savedContent === null || tab.content !== tab.savedContent).length;
  const currentSavingCount = Array.from(savingPaths).filter((path) => path.startsWith(`${workspaceKey}:`)).length;
  const isSaving = savingPaths.has(`${workspaceKey}:${activePath}`);
  const previewSupported = !!activeTab && isPreviewablePath(activeTab.path);
  const previewVisible = !!activeTab && (activeTab.showPreview ?? isPreviewablePath(activeTab.path));
  const selectionRange = editorRef.current?.getSelection();
  const insights = useIdeInsights({
    workspaceId:workspace?.id??"",workspaceName:workspace?.name??"",root:workspace?.root_path??"",
    activeFile:codeOssOpen ? ossContext.path : activeTab?.previewOnly ? undefined : activeTab?.path,
    selectionStart:codeOssOpen ? ossContext.selectionStart : selectionRange&&!selectionRange.isEmpty()?selectionRange.startLineNumber:undefined,
    selectionEnd:codeOssOpen ? ossContext.selectionEnd : selectionRange&&!selectionRange.isEmpty()?selectionRange.endLineNumber:undefined,
    dirty:dirtyCount>0||ossContext.dirty,disabled:agentStarting||agentRunning||(codeOssOpen&&!ossContext.connected),
    disabledReason:codeOssOpen&&!ossContext.connected?"Connecting to the Code OSS editor. Reopen Code OSS if this persists.":undefined,
    provider:providerName,mode:engineeringPrinciplesMode,onShow:()=>{setShowAgentPanel(true);setAgentPanelView("insights");},onApplied:()=>{
      setPreviewReloadKey(v=>v+1);void loadDirectory(".");
      for(const tab of tabs){if(tab.previewOnly||tab.savedContent!==tab.content)continue;void invoke<IdeFile>("workspace_read_file",{workspaceId:workspace?.id,path:tab.path}).then(file=>setTabs(current=>current.map(t=>t.path===file.path&&t.content===t.savedContent?{...t,content:file.content,savedContent:file.content,etag:file.etag}:t))).catch(()=>undefined);}
    }
  });

  useEffect(()=>{
    if(!codeOssStatus?.running||!workspace){setOssContext({selectionText:"",dirty:false,connected:false});return;}
    let gone=false;
    const poll=()=>void invoke<typeof ossContext>("ide_editor_context",{workspaceId:workspace.id}).then(c=>{if(!gone)setOssContext(c);}).catch(()=>undefined);
    const timer=window.setInterval(poll,1000);poll();return()=>{gone=true;window.clearInterval(timer);};
  },[codeOssStatus?.running,workspace?.id]);

  function openInsightLocation(path:string,line:number){
    if(codeOssOpen&&workspace){void invoke("ide_editor_navigate",{workspaceId:workspace.id,path,line}).catch(e=>setEditorError(String(e)));}
    else{void openFile(path).then(()=>setJumpTo({path,line}));}
  }

  function setTabs(nextTabs: IdeTab[] | ((current: IdeTab[]) => IdeTab[])) {
    if (!workspaceKey) return;
    setTabsByWorkspace((current) => {
      const previous = current[workspaceKey] ?? [];
      const next = typeof nextTabs === "function" ? nextTabs(previous) : nextTabs;
      return { ...current, [workspaceKey]: next };
    });
  }

  function setActivePath(path: string) {
    if (workspaceKey) setActivePathsByWorkspace((current) => ({ ...current, [workspaceKey]: path }));
  }

  const loadDirectory = useCallback(async (path: string, append = false) => {
    if (!workspace) return;
    setLoadingDirectories((current) => new Set(current).add(path));
    try {
      const cursor = append ? directories[path]?.next_cursor ?? null : null;
      const page = await invoke<WorkspacePage>("workspace_list_directory", {
        workspaceId: workspace.id,
        path,
        cursor,
        limit: 500,
      });
      if (workspaceKeyRef.current !== workspace.id) return;
      setDirectories((current) => ({
        ...current,
        [path]: {
          entries: append ? [...(current[path]?.entries ?? []), ...page.entries] : page.entries,
          next_cursor: page.next_cursor,
          scanned: page.scanned,
        },
      }));
      setEditorError("");
    } catch (cause) {
      if (workspaceKeyRef.current === workspace.id) setEditorError(String(cause));
    } finally {
      if (workspaceKeyRef.current === workspace.id) {
        setLoadingDirectories((current) => {
          const next = new Set(current);
          next.delete(path);
          return next;
        });
      }
    }
  }, [directories, workspace]);

  useEffect(() => {
    setDirectories({});
    setExpandedDirectories(new Set(["."]));
    setSelectedFolder(".");
    setSearchResults([]);
    setSearchQuery("");
    setSubmittedSearch("");
    setPane("files");
    setEditorError("");
    setSelectedCode("");
    if (workspace?.exists) void loadDirectory(".");
  }, [workspace?.id, workspace?.exists]);

  useEffect(() => {
    setSelectedCode("");
    smartSelectionRef.current = null;
    setSmartSelection(null);
  }, [workspace?.id, activePath]);

  useEffect(() => {
    let cancelled = false;
    setPreviewToken(null);
    if (!workspace?.exists) return () => { cancelled = true; };
    void invoke<string>("ide_workspace_preview_token", { workspaceId: workspace.id })
      .then((token) => { if (!cancelled) setPreviewToken(token); })
      .catch((cause) => { if (!cancelled) setEditorError(`Preview is unavailable: ${String(cause)}`); });
    return () => { cancelled = true; };
  }, [workspace?.id, workspace?.exists]);

  useEffect(() => {
    if (!activeTab || !HTML_PREVIEW_EXTENSIONS.has(fileExtension(activeTab.path))) return;
    const path = activeTab.path;
    const content = activeTab.content;
    const workspaceId = workspaceKey;
    const timer = window.setTimeout(() => setPreviewSource({ workspaceId, path, content }), 180);
    return () => window.clearTimeout(timer);
  }, [workspaceKey, activeTab?.path, activeTab?.content]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void invoke<CodeOssStatus>("ide_code_oss_status").then((status) => {
      if (!disposed) setCodeOssStatus(status);
    }).catch(() => undefined);
    void listen<{ downloaded: number; total: number }>("ide-code-oss-download-progress", (event) => {
      if (!disposed) setCodeOssProgress(event.payload);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!codeOssOpen || !workspace || !codeOssWorkspaceId || codeOssWorkspaceId === workspace.id) return;
    let cancelled = false;
    setCodeOssBusy(true);
    setCodeOssError("");
    void invoke<CodeOssStatus>("ide_code_oss_start", { workspaceId: workspace.id }).then((status) => {
      if (cancelled) return;
      setCodeOssStatus(status);
      setCodeOssUrl(status.url);
      setCodeOssWorkspaceId(workspace.id);
    }).catch((cause) => {
      if (!cancelled) setCodeOssError(String(cause));
    }).finally(() => {
      if (!cancelled) setCodeOssBusy(false);
    });
    return () => { cancelled = true; };
  }, [workspace?.id, codeOssOpen, codeOssWorkspaceId]);

  useEffect(() => {
    if (!isActive || !workspace) return;
    for (const directory of Object.keys(directories)) void loadDirectory(directory);
    for (const tab of tabs) {
      if (tab.previewOnly) continue;
      if (tab.savedContent === null || tab.content !== tab.savedContent) continue;
      void invoke<IdeFile>("workspace_read_file", { workspaceId: workspace.id, path: tab.path }).then((file) => {
        setTabs((current) => current.map((item) => item.path === file.path && item.savedContent === item.content
          ? { ...item, content: file.content, savedContent: file.content, etag: file.etag }
          : item));
      }).catch(() => undefined);
    }
    // Refresh visible files and clean editor tabs when returning from the agent chat.
  }, [isActive, workspace?.id]);

  refreshAgentFilesRef.current = () => {
    if (!workspace) return;
    for (const [directory, existingPage] of Object.entries(directories)) {
      setLoadingDirectories((current) => new Set(current).add(directory));
      void (async () => {
        let cursor: string | null = null;
        let entries: WorkspaceEntry[] = [];
        let nextCursor: string | null = null;
        let scanned = 0;
        try {
          do {
            const page: WorkspacePage = await invoke<WorkspacePage>("workspace_list_directory", {
              workspaceId: workspace.id,
              path: directory,
              cursor,
              limit: 500,
            });
            if (workspaceKeyRef.current !== workspace.id) return;
            entries = [...entries, ...page.entries];
            scanned += page.scanned;
            nextCursor = page.next_cursor ?? null;
            cursor = nextCursor;
          } while (cursor && entries.length < existingPage.entries.length);
          setDirectories((current) => ({
            ...current,
            [directory]: { entries, next_cursor: nextCursor, scanned },
          }));
        } catch (cause) {
          if (workspaceKeyRef.current === workspace.id) setEditorError(String(cause));
        } finally {
          if (workspaceKeyRef.current === workspace.id) {
            setLoadingDirectories((current) => {
              const next = new Set(current);
              next.delete(directory);
              return next;
            });
          }
        }
      })();
    }
    const rootPrefix = `${workspace.root_path.replace(/\/$/, "")}/`;
    for (const change of agentFileChanges) {
      const path = change.path.startsWith(rootPrefix) ? change.path.slice(rootPrefix.length) : change.path;
      if (path.startsWith("/") || path === ".." || path.startsWith("../")) continue;
      const tab = tabs.find((item) => item.path === path);
      if (!tab || tab.previewOnly || tab.savedContent === null || tab.content !== tab.savedContent) continue;
      void invoke<IdeFile>("workspace_read_file", { workspaceId: workspace.id, path }).then((file) => {
        setTabs((current) => current.map((item) => item.path === file.path && item.savedContent === item.content
          ? { ...item, content: file.content, savedContent: file.content, etag: file.etag }
          : item));
      }).catch(() => undefined);
    }
  };

  useEffect(() => {
    if (agentRefreshVersion > 0) {
      setPreviewReloadKey((revision) => revision + 1);
      refreshAgentFilesRef.current();
    }
  }, [agentRefreshVersion, workspace?.id]);

  useEffect(() => {
    const transcript = agentTranscriptRef.current;
    if (transcript) transcript.scrollTop = transcript.scrollHeight;
  }, [agentMessages, agentActivity, agentFileChanges]);

  useEffect(() => {
    const viewport = treeViewportRef.current;
    if (!viewport) return;
    const observer = new ResizeObserver(() => setTreeViewportHeight(viewport.clientHeight));
    observer.observe(viewport);
    setTreeViewportHeight(viewport.clientHeight);
    return () => observer.disconnect();
  }, [workspace?.id, pane]);

  useEffect(() => {
    if (!resizingAgentPanel) return;
    const resize = (event: PointerEvent) => setAgentPanelWidth(Math.max(270, Math.min(620, window.innerWidth - event.clientX)));
    const stop = () => setResizingAgentPanel(false);
    window.addEventListener("pointermove", resize);
    window.addEventListener("pointerup", stop, { once: true });
    window.addEventListener("pointercancel", stop, { once: true });
    return () => {
      window.removeEventListener("pointermove", resize);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
    };
  }, [resizingAgentPanel]);

  useEffect(() => {
    if (!resizingExplorer) return;
    const resize = (event: PointerEvent) => {
      const left = workspaceViewRef.current?.getBoundingClientRect().left ?? 0;
      setExplorerWidth(Math.max(180, Math.min(430, event.clientX - left - 48)));
    };
    const stop = () => setResizingExplorer(false);
    window.addEventListener("pointermove", resize);
    window.addEventListener("pointerup", stop, { once: true });
    window.addEventListener("pointercancel", stop, { once: true });
    return () => {
      window.removeEventListener("pointermove", resize);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
    };
  }, [resizingExplorer]);

  useEffect(() => {
    if (!isActive) return;
    const handleEditorShortcut = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey)) return;
      const key = event.key.toLowerCase();
      if (key === "p" || (key === "f" && event.shiftKey)) {
        event.preventDefault();
        setSearchText(key === "f");
        setPane("search");
        window.setTimeout(() => searchInputRef.current?.focus(), 0);
      }
    };
    const openQuickSearch = () => {
      setSearchText(false);
      setPane("search");
      window.setTimeout(() => searchInputRef.current?.focus(), 0);
    };
    window.addEventListener("keydown", handleEditorShortcut);
    window.addEventListener("jadx:ide-open-quick-search", openQuickSearch);
    return () => {
      window.removeEventListener("keydown", handleEditorShortcut);
      window.removeEventListener("jadx:ide-open-quick-search", openQuickSearch);
    };
  }, [isActive]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview().onDragDropEvent((event) => {
      if (!isActive) return;
      if (event.payload.type === "enter") {
        setFolderDragActive(event.payload.paths.length > 0);
      } else if (event.payload.type === "over") {
        setFolderDragActive(true);
      } else if (event.payload.type === "leave") {
        setFolderDragActive(false);
      } else if (event.payload.type === "drop") {
        setFolderDragActive(false);
        const path = event.payload.paths[0];
        if (path) void onOpenWorkspacePathRef.current(path);
      }
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch((cause) => {
      if (!disposed) setEditorError(`Folder drop is unavailable: ${String(cause)}`);
    });
    return () => {
      disposed = true;
      unlisten?.();
      setFolderDragActive(false);
    };
  }, [isActive]);

  useEffect(() => {
    const query = submittedSearch.trim();
    if (!isActive || !workspace || pane !== "search" || query.length < 2) {
      if (!query) setSearchResults([]);
      setSearchStatus(query.length === 1 && pane === "search" ? "Type one more character to search." : "");
      setSearchingError("");
      setSearching(false);
      return;
    }
    let cancelled = false;
    setSearching(true);
    setSearchingError("");
    void invoke<SearchResponse>("workspace_search_files", {
      workspaceId: workspace.id,
      query,
      searchText,
    }).then((response) => {
      if (cancelled) return;
      setSearchResults(response.results);
      setSearchStatus(`${response.results.length} results · ${response.scannedFiles.toLocaleString()} files scanned${response.bounded ? " · result limit reached" : ""}`);
    }).catch((cause) => {
      if (!cancelled) setSearchingError(String(cause));
    }).finally(() => {
      if (!cancelled) setSearching(false);
    });
    return () => {
      cancelled = true;
    };
  }, [isActive, workspace?.id, pane, submittedSearch, searchText, searchRun]);

  useEffect(() => {
    if (jumpTo && activePath === jumpTo.path && editorRef.current) {
      editorRef.current.revealLineInCenter(jumpTo.line);
      editorRef.current.setPosition({ lineNumber: jumpTo.line, column: 1 });
      editorRef.current.focus();
      setJumpTo(null);
    }
  }, [jumpTo, activePath, activeTab]);

  const treeRows = useMemo(() => {
    const rows: TreeRow[] = [];
    const append = (directory: string, depth: number) => {
      for (const entry of directories[directory]?.entries ?? []) {
        rows.push({ kind: "entry", entry, depth });
        if (entry.kind === "directory" && expandedDirectories.has(entry.path)) {
          append(entry.path, depth + 1);
          if (directories[entry.path]?.next_cursor) rows.push({ kind: "more", path: entry.path, depth: depth + 1 });
        }
      }
    };
    append(".", 0);
    if (directories["."]?.next_cursor) rows.push({ kind: "more", path: ".", depth: 0 });
    return rows;
  }, [directories, expandedDirectories]);

  const treeStart = Math.max(0, Math.floor(treeScrollTop / 30) - 10);
  const treeEnd = Math.min(treeRows.length, Math.ceil((treeScrollTop + treeViewportHeight) / 30) + 10);

  const openFile = useCallback(async (path: string) => {
    const existing = tabs.find((tab) => tab.path === path);
    if (existing) {
      setActivePath(path);
      return;
    }
    if (!workspace) return;
    if (isBinaryPreviewPath(path)) {
      setTabs((current) => current.some((tab) => tab.path === path)
        ? current
        : [...current, { path, content: "", savedContent: "", etag: null, previewOnly: true, showPreview: true }]);
      setActivePath(path);
      setEditorError("");
      return;
    }
    try {
      const file = await invoke<IdeFile>("workspace_read_file", { workspaceId: workspace.id, path });
      setTabs((current) => current.some((tab) => tab.path === path)
        ? current
        : [...current, { path: file.path, content: file.content, savedContent: file.content, etag: file.etag }]);
      setActivePath(path);
      setEditorError("");
    } catch (cause) {
      setEditorError(String(cause));
    }
  }, [tabs, workspace]);

  const saveFile = useCallback(async (path: string) => {
    const tab = tabs.find((item) => item.path === path);
    if (!workspace || !tab || tab.previewOnly || (tab.savedContent !== null && tab.content === tab.savedContent)) return;
    const savingKey = `${workspace.id}:${path}`;
    setSavingPaths((current) => new Set(current).add(savingKey));
    setEditorError("");
    const contentBeingSaved = tab.content;
    try {
      const saved = await invoke<IdeFile>("workspace_save_file", {
        workspaceId: workspace.id,
        path,
        content: contentBeingSaved,
        expectedEtag: tab.etag,
      });
      setTabs((current) => current.map((item) => item.path === path
        ? { ...item, savedContent: contentBeingSaved, etag: saved.etag }
        : item));
      setPreviewReloadKey((revision) => revision + 1);
      const parent = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : ".";
      if (tab.savedContent === null) void loadDirectory(parent);
    } catch (cause) {
      if (workspaceKeyRef.current === workspace.id) setEditorError(String(cause));
    } finally {
      setSavingPaths((current) => {
        const next = new Set(current);
        next.delete(savingKey);
        return next;
      });
    }
  }, [tabs, workspace, loadDirectory]);

  saveActiveRef.current = () => { if (activePath) void saveFile(activePath); };

  const onEditorMount = useCallback((editor: monaco.editor.IStandaloneCodeEditor, editorMonaco: typeof monaco) => {
    editorRef.current = editor;
    editor.addCommand(editorMonaco.KeyMod.CtrlCmd | editorMonaco.KeyCode.KeyS, () => saveActiveRef.current());
    let popupTimer = 0;
    const updateSelection = () => {
      const selection = editor.getSelection();
      const model = editor.getModel();
      const text = selection && model ? model.getValueInRange(selection) : "";
      setSelectedCode(text.length > 12_000 ? `${text.slice(0, 12_000)}\n… selection shortened …` : text);
      if (!selection || selection.isEmpty() || !text.trim() || !model) {
        window.clearTimeout(popupTimer);
        smartSelectionRef.current = null;
        setSmartSelection(null);
        return;
      }

      const editorBounds = editorCanvasRef.current?.getBoundingClientRect();
      const workspaceBounds = workspaceViewRef.current?.getBoundingClientRect();
      const position = editor.getScrolledVisiblePosition(selection.getEndPosition())
        ?? editor.getScrolledVisiblePosition(selection.getStartPosition());
      if (!editorBounds || !workspaceBounds || !position) {
        smartSelectionRef.current = null;
        setSmartSelection(null);
        return;
      }

      const popupWidth = Math.max(180, Math.min(340, editorBounds.width - 16));
      const popupHeight = 164;
      const minLeft = editorBounds.left + 8;
      const maxLeft = Math.max(minLeft, editorBounds.right - popupWidth - 8);
      const left = Math.max(minLeft, Math.min(editorBounds.left + position.left, maxLeft)) - workspaceBounds.left;
      const minTop = editorBounds.top + 8;
      const maxTop = Math.max(minTop, editorBounds.bottom - popupHeight - 8);
      let top = editorBounds.top + position.top + position.height + 8;
      if (top > maxTop) top = editorBounds.top + position.top - popupHeight - 8;
      top = Math.max(minTop, Math.min(top, maxTop)) - workspaceBounds.top;
      const target: SmartSelection = {
        path: activePathRef.current,
        text: text.slice(0, 12_000),
        truncated: text.length > 12_000,
        startLine: selection.startLineNumber,
        endLine: selection.endLineNumber,
        left,
        top,
        width: popupWidth,
      };
      smartSelectionRef.current = target;
      window.clearTimeout(popupTimer);
      if (smartModeRef.current) {
        popupTimer = window.setTimeout(() => setSmartSelection(target), 170);
      } else {
        setSmartSelection(null);
      }
    };
    editor.onDidChangeCursorSelection(updateSelection);
    editor.onDidScrollChange(updateSelection);
    editor.onDidLayoutChange(updateSelection);
  }, []);

  async function saveAll() {
    const dirtyTabs = tabs.filter((tab) => tab.savedContent === null || tab.content !== tab.savedContent);
    for (const tab of dirtyTabs) await saveFile(tab.path);
  }

  function toggleFolder(entry: WorkspaceEntry) {
    const opening = !expandedDirectories.has(entry.path);
    setSelectedFolder(entry.path);
    setExpandedDirectories((current) => {
      const next = new Set(current);
      if (opening) next.add(entry.path);
      else next.delete(entry.path);
      return next;
    });
    if (opening && !directories[entry.path]) void loadDirectory(entry.path);
  }

  function closeTab(path: string) {
    const tab = tabs.find((item) => item.path === path);
    if (tab && (tab.savedContent === null || tab.content !== tab.savedContent) && !window.confirm(`Discard unsaved changes to ${path}?`)) return;
    const remaining = tabs.filter((item) => item.path !== path);
    setTabs(remaining);
    if (activePath === path) setActivePath(remaining[remaining.length - 1]?.path ?? "");
    setSelectedCode("");
  }

  async function switchWorkspace(id: string) {
    await onSelectWorkspace(id);
  }

  async function createFile() {
    if (!workspace) return;
    const base = selectedFolder === "." ? "" : `${selectedFolder}/`;
    const path = window.prompt("New file path", base)?.trim().replace(/^\/+/, "");
    if (!path) return;
    const relative = path.startsWith(base) || !base ? path : `${base}${path}`;
    if (tabs.some((tab) => tab.path === relative)) {
      setActivePath(relative);
      return;
    }
    setTabs((current) => [...current, { path: relative, content: "", savedContent: null, etag: null }]);
    setActivePath(relative);
  }

  async function createFolder() {
    if (!workspace) return;
    const base = selectedFolder === "." ? "" : `${selectedFolder}/`;
    const raw = window.prompt("New folder path", base)?.trim().replace(/^\/+/, "").replace(/\/+$/, "");
    if (!raw) return;
    const relative = raw.startsWith(base) || !base ? raw : `${base}${raw}`;
    try {
      await invoke<string>("workspace_create_directory", { workspaceId: workspace.id, path: relative });
      setEditorError("");
      const parent = relative.includes("/") ? relative.slice(0, relative.lastIndexOf("/")) : ".";
      setExpandedDirectories((current) => new Set(current).add(relative));
      if (!directories[relative]) await loadDirectory(relative);
      void loadDirectory(parent);
    } catch (cause) {
      setEditorError(String(cause));
    }
  }

  async function reloadActiveFile() {
    if (!workspace || !activeTab) return;
    if ((activeTab.savedContent === null || activeTab.content !== activeTab.savedContent) && !window.confirm("Discard editor changes and reload the file from disk?")) return;
    try {
      const file = await invoke<IdeFile>("workspace_read_file", { workspaceId: workspace.id, path: activeTab.path });
      setTabs((current) => current.map((tab) => tab.path === file.path
        ? { ...tab, content: file.content, savedContent: file.content, etag: file.etag }
        : tab));
      setEditorError("");
    } catch (cause) {
      setEditorError(String(cause));
    }
  }

  async function openCodeOss() {
    if (!workspace) return;
    setCodeOssBusy(true);
    setCodeOssError("");
    setCodeOssProgress(null);
    try {
      let status = codeOssStatus;
      if (!status?.installed) {
        status = await invoke<CodeOssStatus>("ide_code_oss_install");
        setCodeOssStatus(status);
      }
      status = await invoke<CodeOssStatus>("ide_code_oss_start", { workspaceId: workspace.id });
      setCodeOssStatus(status);
      setCodeOssUrl(status.url);
      setCodeOssWorkspaceId(workspace.id);
      setCodeOssOpen(Boolean(status.url));
    } catch (cause) {
      setCodeOssError(String(cause));
    } finally {
      setCodeOssBusy(false);
      setCodeOssProgress(null);
    }
  }

  async function stopCodeOss() {
    setCodeOssBusy(true);
    setCodeOssError("");
    try {
      const status = await invoke<CodeOssStatus>("ide_code_oss_stop");
      setCodeOssStatus(status);
      setCodeOssUrl(null);
      setCodeOssOpen(false);
      setCodeOssWorkspaceId(null);
    } catch (cause) {
      setCodeOssError(String(cause));
    } finally {
      setCodeOssBusy(false);
    }
  }

  function runJadX() {
    if (!workspace || insights.busy || ossContext.dirty) return;
    const request = agentPrompt.trim() || (activeTab ? `Review ${activeTab.path} and suggest improvements.` : "Help me understand this project and suggest a useful first improvement.");
    const details = [
      `Selected workspace: ${workspace.name}`,
      codeOssOpen ? `Active file: ${ossContext.path??"none"}` : activeTab ? `Active file: ${activeTab.path}` : "No file is open yet.",
      codeOssOpen&&ossContext.selectionText ? `Code OSS selection from ${ossContext.path}:${ossContext.selectionStart}-${ossContext.selectionEnd}:\n${ossContext.selectionText}` : "",
      includeSelection && selectedCode.trim()
        ? `Selected code from ${activeTab?.path ?? "the editor"}:\n\`\`\`${activeTab?.path.split(".").pop() ?? "text"}\n${selectedCode}\n\`\`\``
        : "",
      ].filter(Boolean).join("\n\n");
    onRunAgent(`${request}\n\n${details}`, { planMode });
  }

  function toggleSmartMode() {
    const next = !smartModeRef.current;
    smartModeRef.current = next;
    setSmartMode(next);
    const selection = smartSelectionRef.current;
    setSmartSelection(next && selection?.path === activePath ? selection : null);
  }

  function submitSmartEdit() {
    if (insights.busy) return;
    const selection = smartSelection;
    const instruction = smartPrompt.trim();
    if (!workspace || !activeTab || !selection || selection.path !== activeTab.path || !instruction || dirtyCount > 0 || agentStarting || agentRunning || !agentToolsEnabled) return;
    const language = fileExtension(selection.path) || "text";
    const selectionNote = selection.truncated
      ? `Selected code excerpt (first 12,000 characters; target lines ${selection.startLine}-${selection.endLine}):`
      : `Selected code (lines ${selection.startLine}-${selection.endLine}):`;
    const request = [
      "Apply this focused Smart Mode edit directly to the selected code in the active workspace.",
      "Treat the selection as the target. Preserve surrounding behavior and formatting, make the smallest correct change, and use JadX's normal edit and approval flow.",
      "Token-efficient workflow: use this request and selection as the main context; inspect only the active file if needed to locate the selection. Read other files only when the requested change clearly depends on them. Avoid broad searches, project summaries, repeated reads, and long explanations.",
      `Workspace: ${workspace.name}`,
      `Target file: ${selection.path}`,
      `User request: ${instruction}`,
      `${selectionNote}\n\`\`\`${language}\n${selection.text}\n\`\`\``,
    ].join("\n\n");
    setSmartSelection(null);
    setSmartPrompt("");
    setShowAgentPanel(true);
    onRunAgent(request, { freshContext: true });
  }

  function workspaceRelativePath(path: string) {
    if (!workspace) return path;
    const rootPrefix = `${workspace.root_path.replace(/\/$/, "")}/`;
    return path.startsWith(rootPrefix) ? path.slice(rootPrefix.length) : path;
  }

  const visibleAgentFileChanges = agentFileChanges.filter((change) => {
    const path = workspaceRelativePath(change.path);
    return !path.startsWith("/") && path !== ".." && !path.startsWith("../");
  });

  function openAgentChangedFile(change: AgentFileChange) {
    const path = workspaceRelativePath(change.path);
    if (!path || path.startsWith("/") || path === ".." || path.startsWith("../")) return;
    setCodeOssOpen(false);
    void openFile(path);
  }

  if (!workspace) {
    return <section className="ide-empty-state">
      <div className="ide-empty-icon"><Code2 size={24} /></div>
      <p className="eyebrow">JADX IDE</p>
      <h1>Open a project to start coding</h1>
      <p>Choose one of your approved workspaces. JadX keeps code access inside that folder and connects refactors to the selected AI provider.</p>
      <div className="ide-empty-actions">
        {workspaces.filter((item) => !item.archived && item.exists).map((item) => <button key={item.id} onClick={() => void switchWorkspace(item.id)}><FolderOpen size={15} /> {item.name}</button>)}
        <button className="ide-primary-action" onClick={() => void onAddWorkspace()}><FilePlus2 size={15} /> Open project folder…</button>
      </div>
    </section>;
  }

  const visibleRows = treeRows.slice(treeStart, treeEnd);
  const lineCount = activeTab?.content.split("\n").length ?? 0;
  const activePreviewUrl = activeTab && previewToken ? workspacePreviewUrl(previewToken, activeTab.path) : "";
    const previewContent = activeTab && previewSource?.workspaceId === workspaceKey && previewSource.path === activeTab.path ? previewSource.content : activeTab?.content ?? "";
  const previewDocument = activeTab && HTML_PREVIEW_EXTENSIONS.has(fileExtension(activeTab.path)) && previewToken
    ? htmlPreviewDocument(previewContent, workspacePreviewDirectoryUrl(previewToken, activeTab.path))
    : "";

  function renderPreview() {
    if (!activeTab) return null;
    if (!previewToken) return <div className="ide-preview-message"><LoaderCircle size={15} className="ide-spin" /> Preparing secure preview…</div>;
    if (HTML_PREVIEW_EXTENSIONS.has(fileExtension(activeTab.path))) {
      return <iframe key={`${activeTab.path}:${previewReloadKey}`} className="ide-live-preview-frame" title={`Live preview of ${activeTab.path}`} srcDoc={previewDocument} sandbox="allow-scripts" referrerPolicy="no-referrer" />;
    }
    if (fileExtension(activeTab.path) === "pdf") {
      return <iframe key={`${activeTab.path}:${previewReloadKey}`} className="ide-live-preview-frame" title={`Preview of ${activeTab.path}`} src={activePreviewUrl} referrerPolicy="no-referrer" />;
    }
    if (IMAGE_PREVIEW_EXTENSIONS.has(fileExtension(activeTab.path))) {
      return <div className="ide-image-preview"><img key={`${activeTab.path}:${previewReloadKey}`} src={activePreviewUrl} alt={`Preview of ${activeTab.path}`} /></div>;
    }
    return <div className="ide-preview-message">Preview is not available for this file type.</div>;
  }

  const currentContextCost = providerId === "local"
    ? null
    : estimateUsageCost(providerId, modelName, { input_tokens: contextLength }, usagePricing);

  return <section ref={workspaceViewRef} className={`ide-workspace ${engineeringPrinciplesMode ? "engineering-principles-mode" : ""} ${!showExplorer ? "explorer-collapsed" : ""} ${codeOssOpen ? "code-oss-mode" : ""} ${folderDragActive ? "folder-drag-active" : ""}`} style={{ "--ide-explorer-width": `${explorerWidth}px` } as CSSProperties} aria-label="JadX IDE">
    {insights.dialogs}
    <div className="ide-activity-rail" aria-label="IDE views">
      <button className={pane === "files" && showExplorer ? "selected" : ""} onClick={() => { setPane("files"); setShowExplorer(true); }} aria-label="Show project files" title="Explorer"><Files size={18} /></button>
      <button className={pane === "search" && showExplorer ? "selected" : ""} onClick={() => { setPane("search"); setShowExplorer(true); }} aria-label="Search project" title="Search"><Search size={18} /></button>
      {!showExplorer && <button onClick={() => setShowExplorer(true)} aria-label="Show Explorer sidebar" title="Show Explorer"><PanelLeftOpen size={17} /></button>}
      <span className="ide-rail-spacer" />
      <button onClick={() => setShowAgentPanel((value) => !value)} aria-label={showAgentPanel ? "Hide AI panel" : "Show AI panel"} title="JadX Agent">{showAgentPanel ? <PanelRightClose size={17} /> : <PanelRightOpen size={17} />}</button>
    </div>

    {showExplorer && <>
    <aside className="ide-explorer">
      <header className="ide-explorer-title">
        <div><span>{pane === "files" ? "EXPLORER" : "SEARCH"}</span><strong>{workspace.name}</strong></div>
        <div className="ide-explorer-actions">
          <button onClick={() => void createFile()} disabled={!workspace.exists} title="New file" aria-label="New file"><FilePlus2 size={15} /></button>
          <button onClick={() => void createFolder()} disabled={!workspace.exists} title="New folder" aria-label="New folder"><FolderPlus size={15} /></button>
          <button onClick={() => { setDirectories({}); setExpandedDirectories(new Set(["." ])); void loadDirectory("."); }} title="Refresh project files" aria-label="Refresh project files"><RefreshCw size={14} /></button>
          <button onClick={() => setShowExplorer(false)} title="Hide Explorer sidebar" aria-label="Hide Explorer sidebar"><PanelLeftClose size={15} /></button>
        </div>
      </header>
      <label className="ide-project-picker">
        <FolderOpen size={14} />
        <select value={workspace.id} onChange={(event) => void switchWorkspace(event.target.value)} aria-label="Project workspace">
          {workspaces.filter((item) => !item.archived && item.exists).map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
        </select>
        <ChevronDown size={13} />
      </label>

      {pane === "files" ? <>
        <div className="ide-tree-caption"><span>{workspace.root_path}</span></div>
        <div className="ide-tree-viewport" ref={treeViewportRef} onScroll={(event) => setTreeScrollTop(event.currentTarget.scrollTop)}>
          {loadingDirectories.has(".") && !directories["."] ? <div className="ide-tree-state"><LoaderCircle size={14} className="ide-spin" /> Reading project…</div>
            : treeRows.length === 0 ? <div className="ide-tree-state">No project files found.</div>
              : <div className="ide-tree-spacer" style={{ height: treeRows.length * 30 }}>
                {visibleRows.map((row, index) => {
                  const rowIndex = treeStart + index;
                  if (row.kind === "more") {
                    return <button key={`more-${row.path}`} className="ide-tree-more" style={{ top: rowIndex * 30, paddingLeft: 13 + row.depth * 14 }} disabled={loadingDirectories.has(row.path)} onClick={() => void loadDirectory(row.path, true)}>
                      {loadingDirectories.has(row.path) ? <LoaderCircle size={13} className="ide-spin" /> : <RefreshCw size={12} />} Load more…
                    </button>;
                  }
                  const directory = row.entry.kind === "directory";
                  const isExpanded = expandedDirectories.has(row.entry.path);
                  const selected = activePath === row.entry.path || selectedFolder === row.entry.path;
                  return <button key={row.entry.path} className={`ide-tree-row ${selected ? "selected" : ""} ${directory ? "is-directory" : ""}`} style={{ top: rowIndex * 30, paddingLeft: 9 + row.depth * 14 }} onClick={() => directory ? toggleFolder(row.entry) : void openFile(row.entry.path)} title={row.entry.path}>
                    {directory ? isExpanded ? <ChevronDown size={13} /> : <ChevronRight size={13} /> : <span className="ide-tree-indent" />}
                    {directory ? isExpanded ? <FolderOpen size={14} /> : <Folder size={14} /> : <FileCode2 size={14} />}
                    <span>{row.entry.path.split("/").pop()}</span>
                  </button>;
                })}
              </div>}
        </div>
      </> : <div className="ide-search-pane">
        <form className="ide-search-input" onSubmit={(event) => { event.preventDefault(); setSearchResults([]); setSearchStatus(""); setSubmittedSearch(searchQuery); setSearchRun((run) => run + 1); }}><Search size={14} /><input ref={searchInputRef} autoFocus value={searchQuery} onChange={(event) => setSearchQuery(event.target.value)} placeholder="Search files or text" /><kbd>{searchText ? "⌘⇧F" : "⌘P"}</kbd><button type="submit" aria-label="Search workspace" title="Search workspace"><Send size={13} /></button></form>
        <label className="ide-search-option"><input type="checkbox" checked={searchText} onChange={(event) => setSearchText(event.target.checked)} /> Search inside files</label>
        <div className="ide-search-status">{searching ? "Searching workspace…" : searchStatus || (searchQuery.trim().length < 2 ? "Type at least 2 characters. Search scans up to 100,000 files and skips ignored/generated folders." : "Press Enter to search. Search scans 100,000 files max; text search reads UTF-8 files up to 1 MB, with a 256 MB total read cap.")}</div>
        {searchingError && <div className="ide-search-error">{searchingError}</div>}
        <div className="ide-search-results">{searchResults.map((result) => <button key={`${result.path}-${result.line ?? 0}`} className="ide-search-result" onClick={() => { void openFile(result.path).then(() => { if (result.line) setJumpTo({ path: result.path, line: result.line }); }); }}>
          <span><FileCode2 size={13} />{result.path}{result.line ? <b>:{result.line}</b> : null}</span>
          {result.preview && <small>{result.preview}</small>}
        </button>)}</div>
      </div>}
      <footer className="ide-explorer-footer"><span><span className={`ide-health-dot ${workspace.exists ? "" : "offline"}`} /> {workspace.exists ? "Workspace ready" : "Folder unavailable"}</span><span>Lazy tree</span></footer>
    </aside>
    <div className={`ide-explorer-resize ${resizingExplorer ? "resizing" : ""}`} role="separator" aria-label="Resize Explorer" aria-orientation="vertical" aria-valuemin={180} aria-valuemax={430} aria-valuenow={explorerWidth} tabIndex={0} onPointerDown={(event) => { event.preventDefault(); setResizingExplorer(true); }} onKeyDown={(event) => { if (event.key === "ArrowLeft") setExplorerWidth((width) => Math.max(180, width - 16)); if (event.key === "ArrowRight") setExplorerWidth((width) => Math.min(430, width + 16)); }} />
    </>}

    <div className="ide-main-area">
      <div className="ide-tab-bar">
        {codeOssOpen ? <div className="ide-code-oss-title"><Code2 size={14} /><strong>Code OSS</strong><span>Open VSX extensions</span></div> : <div className="ide-tabs">{tabs.map((tab) => {
          const dirty = tab.savedContent === null || tab.content !== tab.savedContent;
          return <button key={tab.path} className={`ide-tab ${activePath === tab.path ? "active" : ""}`} onClick={() => setActivePath(tab.path)} title={tab.path}>
            <FileCode2 size={13} /><span>{tab.path.split("/").pop()}</span>{dirty && <i aria-label="Unsaved changes" />}
            <b onClick={(event) => { event.stopPropagation(); closeTab(tab.path); }} aria-label={`Close ${tab.path}`}><X size={13} /></b>
          </button>;
        })}</div>}
        <div className="ide-tab-actions"><AiAnalysisToolbar ai={insights} disabled={agentStarting||agentRunning} onShow={()=>setShowAgentPanel(true)}/>{!codeOssOpen && <span className={`ide-provider-badge ${providerId === "local" ? "local" : "cloud"}`} title={providerId === "local" ? `On-device inference · ${contextLength.toLocaleString()} configured context of ${contextMaximum.toLocaleString()} maximum · strongest reasoning when supported` : `External API request · ${contextLength.toLocaleString()} configured context of ${contextMaximum.toLocaleString()} maximum · selected project source may be sent to ${providerName}${currentContextCost ? ` · full-window input estimate ${formatUsd(currentContextCost.amount)}` : " · no price configured"}`}>
          {providerId === "local" ? <HardDrive size={12}/> : <Cloud size={12}/>}<b>{providerId === "local" ? "ON DEVICE" : "CLOUD"}</b><span>{providerName} · {modelName || "Choose model"}</span><i>{Math.round(contextLength / 1024)}K/{Math.round(contextMaximum / 1024)}K</i>{supportsMaxReasoning&&<em>MAX</em>}{providerId !== "local"&&<em>{currentContextCost ? `${formatUsd(currentContextCost.amount)} in` : "rate n/a"}</em>}
        </span>}
          {!codeOssOpen && <button className={`ide-runtime-button ide-smart-mode-button ${smartMode ? "active" : ""}`} aria-pressed={smartMode} disabled={!activeTab || activeTab.previewOnly || !agentToolsEnabled} onClick={toggleSmartMode} title={agentToolsEnabled ? "Select code, then describe a focused edit" : agentToolLimitation ?? "Agent tools are unavailable for this model"}><Sparkles size={13} />Smart Mode</button>}
          {!codeOssOpen && previewSupported && <button className="ide-runtime-button" onClick={() => activeTab && setTabs((current) => current.map((tab) => tab.path === activeTab.path ? { ...tab, showPreview: !previewVisible } : tab))} title={previewVisible ? "Hide file preview" : "Show file preview"}><Eye size={13} />{previewVisible ? "Hide Preview" : "Preview"}</button>}
          {!codeOssOpen && activeTab?.previewOnly && previewVisible && <button className="ide-runtime-button" onClick={() => setPreviewReloadKey((revision) => revision + 1)} title="Refresh preview"><RefreshCw size={13} />Refresh</button>}
          {!codeOssOpen && activeTab && <button className="ide-runtime-button ide-icon-button" onClick={() => void invoke("reveal_workspace_file", { path: activeTab.path, workspace: workspace.root_path }).catch((cause) => setEditorError(String(cause)))} title="Reveal the active file in Finder" aria-label="Reveal the active file in Finder"><FolderOpen size={13} /></button>}
          {codeOssOpen && <button className="ide-runtime-button" onClick={() => setCodeOssOpen(false)}>Use JadX editor</button>}
          <button className="ide-runtime-button" onClick={() => codeOssOpen ? void stopCodeOss() : void openCodeOss()} disabled={codeOssBusy || !workspace.exists} title={codeOssStatus?.installed ? "Launch the Code OSS workbench" : "Download and enable the Code OSS workbench"}>
            {codeOssBusy ? <LoaderCircle size={13} className="ide-spin" /> : <Code2 size={13} />}
            {codeOssBusy ? codeOssProgress ? `Downloading ${Math.round(codeOssProgress.downloaded / 1_048_576)} / ${Math.round(codeOssProgress.total / 1_048_576)} MB` : "Starting…" : codeOssOpen ? "Stop Code OSS" : codeOssStatus?.installed ? "Open Code OSS" : "Enable Code OSS"}
          </button>
        </div>
      </div>

      <div className="ide-editor-row">
        <div className="ide-editor-column">
          {editorError && <div className="ide-error-banner" role="alert"><span>{editorError}</span>{editorError.includes("changed on disk") && <button onClick={() => void reloadActiveFile()}>Reload</button>}<button onClick={() => setEditorError("")} aria-label="Dismiss error"><X size={13} /></button></div>}
          {codeOssError && <div className="ide-code-oss-error" role="alert"><span>{codeOssError}</span><button onClick={() => setCodeOssError("")} aria-label="Dismiss Code OSS error"><X size={13} /></button></div>}
          {!codeOssOpen && <div className="ide-code-oss-prompt">
            <div><strong>Full VS Code workbench</strong><span>{codeOssStatus?.installed ? "Ready to open." : "One-time download: about 205 MB."} Extensions can run code with your macOS account permissions. The no-login service listens on this Mac only.</span></div>
            <button onClick={() => void openCodeOss()} disabled={codeOssBusy || !workspace.exists}>{codeOssBusy ? <LoaderCircle size={13} className="ide-spin" /> : <Code2 size={13} />}{codeOssStatus?.installed ? "Open Code OSS" : "Install and open"}</button>
          </div>}
          {codeOssOpen && codeOssUrl ? <iframe className="ide-code-oss-frame" src={codeOssUrl} title="Code OSS workbench" allow="clipboard-read; clipboard-write" referrerPolicy="no-referrer" /> : activeTab?.previewOnly ? <div className="ide-preview-only">{previewVisible ? renderPreview() : <div className="ide-preview-message">Preview hidden. Select Preview to view this file.</div>}</div> : activeTab ? <div className={`ide-editor-workbench ${previewVisible ? "with-preview" : ""}`}>
            <div className="ide-editor-canvas" ref={editorCanvasRef}><Editor
              path={`${workspace.id}/${activeTab.path}`}
              value={activeTab.content}
              onChange={(value) => setTabs((current) => current.map((tab) => tab.path === activePath ? { ...tab, content: value ?? "" } : tab))}
              onMount={onEditorMount}
              theme={engineeringPrinciplesMode ? "jadx-engineering-principles" : "vs-dark"}
              loading={<div className="ide-loading-editor"><LoaderCircle size={16} className="ide-spin" /> Starting code editor…</div>}
              options={{
                automaticLayout: true,
                fontSize: 13,
                fontFamily: "'SF Mono', Menlo, Monaco, monospace",
                minimap: { enabled: true, scale: 1 },
                scrollBeyondLastLine: false,
                smoothScrolling: true,
                renderLineHighlight: "all",
                wordWrap: "off",
                tabSize: 2,
                stickyScroll: { enabled: true },
                padding: { top: 14, bottom: 18 },
                quickSuggestions: { other: true, comments: false, strings: true },
              }}
            /></div>
            {previewVisible && <section className="ide-preview-pane" aria-label="File preview"><header><Eye size={13} /><span>{HTML_PREVIEW_EXTENSIONS.has(fileExtension(activeTab.path)) ? "LIVE PREVIEW" : "PREVIEW"}</span><small>{activeTab.path.split("/").pop()}</small><button onClick={() => setPreviewReloadKey((revision) => revision + 1)} title="Refresh preview" aria-label="Refresh preview"><RefreshCw size={12} /></button></header>{renderPreview()}</section>}
          </div> : <div className="ide-editor-empty">
            <div className="ide-empty-icon"><Code2 size={23} /></div><h2>{workspace.name}</h2>
            <p>Open a file from the explorer, or ask JadX to map the project before changing code.</p>
            <div><button onClick={() => setShowAgentPanel(true)}><MessageSquareText size={14} /> Ask JadX about this project</button><button onClick={() => void createFile()}><FilePlus2 size={14} /> New file</button></div>
          </div>}
          {!codeOssOpen && <footer className="ide-status-bar"><span><Code2 size={13} /> {activeTab ? activeTab.path.split(".").pop()?.toUpperCase() ?? "TEXT" : "JADX IDE"}</span><span>{activeTab ? activeTab.previewOnly ? "Binary preview · read only" : `${lineCount.toLocaleString()} lines · UTF-8` : workspace.root_path}</span><button disabled={!activeTab || activeTab.previewOnly || currentSavingCount > 0 || dirtyCount === 0} onClick={() => void (dirtyCount > 1 ? saveAll() : activeTab && saveFile(activeTab.path))}>{currentSavingCount > 0 ? <LoaderCircle size={13} className="ide-spin" /> : dirtyCount ? <FileCode2 size={13} /> : <Check size={13} />}{currentSavingCount > 0 ? "Saving…" : dirtyCount ? `Save${dirtyCount > 1 ? ` all (${dirtyCount})` : ""} · ⌘S` : "Saved"}</button></footer>}
        </div>

        {showAgentPanel && <>
          <div className={`ide-agent-resize ${resizingAgentPanel ? "resizing" : ""}`} role="separator" aria-label="Resize JadX Agent panel" aria-orientation="vertical" aria-valuemin={270} aria-valuemax={620} aria-valuenow={agentPanelWidth} tabIndex={0} onPointerDown={(event) => { event.preventDefault(); setResizingAgentPanel(true); }} onKeyDown={(event) => { if (event.key === "ArrowLeft") setAgentPanelWidth((width) => Math.min(620, width + 20)); if (event.key === "ArrowRight") setAgentPanelWidth((width) => Math.max(270, width - 20)); }} />
          <aside className="ide-agent-panel" style={{ width: agentPanelWidth, flexBasis: agentPanelWidth }}>
          <div className="ide-ai-panel-tabs"><button className={agentPanelView==="agent"?"selected":""} onClick={()=>setAgentPanelView("agent")}><MessageSquareText size={13}/>Agent</button><button className={agentPanelView==="insights"?"selected":""} onClick={()=>setAgentPanelView("insights")}><Sparkles size={13}/>Insights{insights.report?` · ${insights.report.findings.length}`:""}</button><button onClick={()=>setShowAgentPanel(false)} aria-label="Close AI sidebar"><PanelRightClose size={14}/></button></div>
          {agentPanelView==="insights" ? <AiInsightsPanel ai={insights} onOpen={openInsightLocation} onProfile={onInspectPrinciples} onFollowUp={prompt=>{setAgentPanelView("agent");onRunAgent(prompt,{freshContext:true});}}/> : <>
          <div className="ide-agent-chat-nav" aria-label="Workspace chats">
            <label title="Chats in this workspace"><MessageSquareText size={13}/><select aria-label="Chats in this workspace" value={activeConversationId ?? ""} disabled={agentStarting || agentRunning} onChange={(event) => { if (event.target.value) void onSelectWorkspaceChat(event.target.value); }}>
              {workspaceChats.length === 0 && <option value={activeConversationId ?? ""}>No other workspace chats</option>}
              {workspaceChats.map((chat) => <option key={chat.id} value={chat.id}>{chat.title || "New chat"}</option>)}
            </select><ChevronDown size={12}/></label>
            <button onClick={() => void onNewWorkspaceChat()} disabled={!workspace || agentStarting || agentRunning} title={workspace ? `New chat in ${workspace.name}` : "Open a workspace first"}><FilePlus2 size={13}/>New</button>
          </div>
          <div className="ide-agent-body" ref={agentTranscriptRef}>
            <div className={`ide-agent-mode-row ${executionMode === "DeveloperUnrestricted" ? "danger" : ""}`} title={PERMISSION_DESCRIPTIONS[executionMode]}>
              {executionMode === "DeveloperUnrestricted" ? <ShieldAlert size={13} /> : <ShieldCheck size={13} />}
              <select aria-label="Agent permission mode" value={executionMode} disabled={agentStarting || agentRunning} onChange={(event) => onExecutionMode(event.target.value as ExecutionMode)}>
                <option value="Safe">Safe mode</option>
                <option value="ApproveForMe">Approve for me</option>
                <option value="DeveloperUnrestricted">Full access</option>
              </select>
            </div>
            {executionMode === "DeveloperUnrestricted" && <p className="ide-agent-mode-warning">Edits and commands run without asking, as your user. They never elevate privileges.</p>}
            {securityPrompts && <div className="ide-agent-prompts">{securityPrompts}</div>}
            {agentError && <div className="ide-agent-error" role="alert"><span>{agentError}</span><button onClick={onDismissAgentError} aria-label="Dismiss agent error"><X size={13} /></button></div>}
            {cloudFallback && <section className="ide-cloud-fallback" role="alertdialog" aria-labelledby="ide-cloud-fallback-title">
              <header><div><Cloud size={15}/><strong id="ide-cloud-fallback-title">Try this task with a cloud model?</strong></div><button onClick={onDismissCloudFallback} aria-label="Dismiss cloud fallback"><X size={13}/></button></header>
              <p className="ide-cloud-fallback-reason">{cloudFallback.reason.slice(0, 260)}. No project source has been sent to a cloud provider.</p>
              <details><summary>What would be sent?</summary><p>Your request and any eligible workspace source the agent reads through JadX tools. Hidden, generated, dependency and detected-secret content remains excluded or redacted. Cloud processing can incur provider charges.</p></details>
              {cloudFallback.providers.length ? <div className="ide-cloud-fallback-options">{cloudFallback.providers.map((provider) => {
                const cost = estimateUsageCost(provider.id, provider.model, { input_tokens: provider.maxContext }, usagePricing);
                return <button key={provider.id} onClick={() => onApproveCloudFallback(provider.id)} disabled={agentStarting||agentRunning}>
                  <span><strong>Continue with {provider.name}</strong><small>{provider.model} · {Math.round(provider.maxContext / 1024)}K maximum context</small></span>
                  <small>{cost ? `≈ ${formatUsd(cost.amount)} full-window input` : "No price configured"}</small>
                </button>;
              })}</div> : <p className="ide-cloud-fallback-unconfigured">No acknowledged external model is configured. Set one up in Providers first; this task will stay local.</p>}
              {cloudFallback.providers.length > 0 && <small className="ide-cloud-fallback-cost-note">Estimate assumes a full input window; output is billed separately. Actual usage depends on files read and tokens returned.</small>}
              <footer><button onClick={onOpenProviderSettings}>Provider settings</button><button onClick={onDismissCloudFallback}>Stay local</button></footer>
            </section>}
            {agentPlan}
            {agentTodo}
            {visibleAgentFileChanges.length > 0 && (() => {
              const added = visibleAgentFileChanges.reduce((total, change) => total + change.added_lines, 0);
              const removed = visibleAgentFileChanges.reduce((total, change) => total + change.removed_lines, 0);
              const canUndo = visibleAgentFileChanges.every((change) => Boolean(change.recovery_snapshot));
              return <section className="ide-agent-changes" aria-label="Files changed by JadX">
                <div className="ide-agent-changes-head">
                  <strong>Edited {visibleAgentFileChanges.length} {visibleAgentFileChanges.length === 1 ? "file" : "files"}</strong>
                  <span className="ide-diff-summary"><b className="ide-diff-added">+{added}</b><b className="ide-diff-removed">−{removed}</b></span>
                  <span className="ide-agent-changes-actions">
                    <button onClick={() => onReviewAgentChanges(visibleAgentFileChanges)} title="Review the diffs">View changes</button>
                    <button disabled={!canUndo} title={canUndo ? "Restore the pre-write snapshots" : "Undo is available for existing files that were edited with recovery snapshots"} onClick={() => onUndoAgentChanges(visibleAgentFileChanges)}>Undo</button>
                  </span>
                </div>
                <div className="ide-agent-changes-list">
                  {visibleAgentFileChanges.slice(-10).map((change, index) => <button key={`${change.path}-${index}`} className="ide-agent-change-row" onClick={() => openAgentChangedFile(change)} title="Open in editor">
                    <span>{workspaceRelativePath(change.path)}</span>
                    <small><b className="ide-diff-added">+{change.added_lines}</b> <b className="ide-diff-removed">−{change.removed_lines}</b></small>
                  </button>)}
                </div>
              </section>;
            })()}
            {agentVisualPreview && <section className="ide-agent-visual-preview" aria-label="Visual verification screenshot">
              <div><strong><Eye size={13} /> Visual check</strong><button type="button" onClick={onDismissAgentVisualPreview} aria-label="Dismiss visual preview" title="Dismiss"><X size={13} /></button></div>
              <img src={agentVisualPreview.data_url} alt={`Rendered workspace preview: ${agentVisualPreview.name}`} />
              <small>{agentVisualPreview.name}</small>
            </section>}
            <div className="ide-agent-transcript">
              {agentMessages.length > 0 ? agentMessages.map((message, index) => {
                const live = Boolean(message.streaming);
                const visibleContent = message.content.length > 16000 ? `${message.content.slice(0, 16000)}\n…` : message.content;
                return <article key={`${message.id}-${index}`} className={`ide-agent-message ${message.role === "user" ? "from-user" : "from-agent"}`}>
                  {message.role === "user" ? <strong>You</strong> : null}
                  {message.content ? (message.role === "user"
                    ? <pre>{visibleContent}</pre>
                    : <div className="markdown ide-agent-markdown"><ReactMarkdown remarkPlugins={[remarkGfm]}>{visibleContent}</ReactMarkdown></div>) : null}
                  {live && !message.content ? <div className="ide-agent-activity" aria-label="Agent activity">{agentActivity.slice(-3).map((item, index) => <div key={`${item}-${index}`}><Activity size={12} /><span>{item}</span><i className="ide-agent-live-dot" /></div>)}{agentActivity.length === 0 && <div><Activity size={12} /><span>Preparing first workspace action…</span><i className="ide-agent-live-dot" /></div>}</div> : null}
                </article>;
              }) : <div className="ide-agent-intro"><span className="ide-agent-orb"><Code2 size={21} /></span><h3>Work across your project</h3><p>Ask JadX to trace code, plan a refactor, or make coordinated edits. Changes use JadX’s existing approval and recovery flow.</p></div>}
              {agentActivity.length > 0 && <div className="ide-agent-activity" aria-label="Agent activity">{agentActivity.slice(-5).map((item, index) => <div key={`${item}-${index}`}><Activity size={12} /><span>{item}</span>{(agentRunning || agentStarting) && index === agentActivity.slice(-5).length - 1 && <i className="ide-agent-live-dot" />}</div>)}</div>}
            </div>
            {selectedCode.trim() && <label className="ide-selection-toggle"><input type="checkbox" checked={includeSelection} onChange={(event) => setIncludeSelection(event.target.checked)} /><span>Include editor selection ({selectedCode.split("\n").length} lines)</span></label>}
          </div>
          <form className="ide-agent-composer" onSubmit={(event) => { event.preventDefault(); if (!dirtyCount && !agentStarting && !agentRunning && agentToolsEnabled) runJadX(); }}>
            <textarea value={agentPrompt} onChange={(event) => setAgentPrompt(event.target.value)} placeholder="Ask about this project or request a refactor…" rows={4} />
            <div className="ide-agent-composer-foot"><span>{dirtyCount ? <><span className="ide-dirty-dot" /> Save {dirtyCount} unsaved file{dirtyCount === 1 ? "" : "s"} first</> : !agentToolsEnabled ? agentToolLimitation ?? "Agent tools unavailable" : null}</span><span className="ide-agent-composer-actions">{agentRunning ? <button type="button" className="ide-agent-stop" onClick={onStopAgent}><X size={13} /> Stop</button> : <><button type="button" className={`ide-agent-plan-toggle ${planMode ? "active" : ""}`} onClick={() => setPlanMode((value) => !value)} aria-pressed={planMode} title={planMode ? "Plan mode is on: JadX proposes an approved plan before changing files" : "Plan mode is off: JadX implements directly through the normal approval flow"}><ListTodo size={13} /> Plan</button><button type="submit" disabled={!workspace.exists || dirtyCount > 0 || agentStarting || !agentToolsEnabled} title={!agentToolsEnabled ? agentToolLimitation ?? "Agent tools unavailable for this model" : undefined}><Send size={14} /> {agentStarting ? "Starting…" : planMode ? "Propose plan" : "Run with JadX"}</button></>}</span></div>
          </form>
          </>}
          </aside>
        </>}
      </div>
    </div>
    {smartMode && smartSelection && activeTab && smartSelection.path === activeTab.path && !codeOssOpen && <form
      className="ide-smart-popup"
      style={{ left: smartSelection.left, top: smartSelection.top, width: smartSelection.width }}
      onSubmit={(event) => { event.preventDefault(); submitSmartEdit(); }}
      onKeyDown={(event) => {
        if (event.key === "Escape") { event.preventDefault(); setSmartSelection(null); }
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); submitSmartEdit(); }
      }}
      aria-label="Smart Mode edit prompt"
    >
      <header><Sparkles size={13} /><strong>Smart Mode</strong><span>{smartSelection.path.split("/").pop()}:{smartSelection.startLine}{smartSelection.endLine !== smartSelection.startLine ? `–${smartSelection.endLine}` : ""}</span><button type="button" onClick={() => setSmartSelection(null)} aria-label="Close Smart Mode prompt" title="Close"><X size={13} /></button></header>
      <textarea value={smartPrompt} onChange={(event) => setSmartPrompt(event.target.value)} placeholder="Describe the change to this selection…" rows={2} maxLength={2000} />
      {dirtyCount > 0 && <small className="ide-smart-hint">Save editor changes before applying an AI edit.</small>}
      {smartSelection.truncated && <small className="ide-smart-hint">Large selection excerpt sent; JadX will locate the full range in this file.</small>}
      <footer><small>Focused context · output fits the model window · ⌘↵ to run</small><button type="submit" disabled={!smartPrompt.trim() || !workspace.exists || dirtyCount > 0 || agentStarting || agentRunning || !agentToolsEnabled}>{agentStarting || agentRunning ? <LoaderCircle size={13} className="ide-spin" /> : <Send size={13} />}{agentStarting || agentRunning ? "Working…" : "Apply edit"}</button></footer>
    </form>}
    {folderDragActive && <div className="ide-folder-drop-overlay" aria-live="polite"><div><FolderOpen size={26} /><strong>Open project folder</strong><span>Drop a folder anywhere in JadX to add it as the active workspace.</span></div></div>}
  </section>;
}
