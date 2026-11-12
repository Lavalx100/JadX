import { DiffEditor } from "@monaco-editor/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { BookOpen, Bug, Check, ChevronDown, ChevronRight, FlaskConical, GitCompareArrows, Info, LoaderCircle, Network, Search, ShieldCheck, Sparkles, Square, WandSparkles, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import "./ideIntelligence.css";

export type AnalysisAction = "bugs" | "improvements" | "refactor" | "architecture" | "explain" | "tests";
export const ANALYSIS_ACTIONS = [
  { id: "bugs", label: "Scan for Bugs", icon: Bug, description: "Find logic errors, runtime failures, edge cases and security vulnerabilities." },
  { id: "improvements", label: "Scan for Improvements", icon: Sparkles, description: "Rank performance, structure, maintainability and UX improvements by impact, effort and confidence." },
  { id: "refactor", label: "Smart Refactor", icon: WandSparkles, description: "Propose focused refactors that preserve behavior. Review exact changes in a side-by-side diff." },
  { id: "architecture", label: "Architecture Review", icon: Network, description: "Understand components, dependencies, data flow and practical architectural trade-offs." },
  { id: "explain", label: "Explain Code", icon: BookOpen, description: "Explain the current selection or file in its project context, including limitations and improvements." },
  { id: "tests", label: "Generate Tests", icon: FlaskConical, description: "Propose meaningful tests using the project's existing framework, then review and run them with approval." },
] as const;
export type InsightFinding = { id: string; title: string; category: string; severity: string; path?: string; line?: number; endLine?: number; explanation: string; evidence: string; suggestion: string; verification: string; impact: number; effort: string; confidence: number; followUp: string; evaluation?: Record<string,string>; patchId?: string };
export type InsightPatch = { id: string; title: string; status: string; files: { path: string; before: string; after: string; expectedEtag?: string }[] };
export type InsightReport = { id: string; workspaceId: string; action: string; status: string; message: string; provider: string; model: string; engineeringPrinciples: boolean; summaries: string[]; findings: InsightFinding[]; patches: InsightPatch[]; coverage: { indexedFiles: number; analyzedFiles: number; excludedFiles: number; redactedFiles: number; completedBatches: number; totalBatches: number; indexBounded: boolean; contextTokens: number; inputTokens: number; outputTokens: number; usageReported: boolean }; warnings: string[]; testCommands: {id:string;label:string;command:string}[]; testOutput?: string };
type Context = { workspaceId: string; workspaceName: string; root: string; activeFile?: string; selectionStart?: number; selectionEnd?: number; dirty: boolean; disabled: boolean; disabledReason?: string; provider: string; mode: boolean; onShow: () => void; onApplied: () => void };
const BUSY = new Set(["indexing", "running", "applying", "testing"]);

export function useIdeInsights(context: Context) {
  const [reports, setReports] = useState<Record<string,InsightReport>>({});
  const [scope, setScope] = useState<"file"|"project">("file");
  const [setupAction, setSetupAction] = useState<AnalysisAction|null>(null);
  const [question, setQuestion] = useState("");
  const [error, setError] = useState("");
  const [starting, setStarting] = useState(false);
  const [reviewIds, setReviewIds] = useState<string[]>([]);
  const [reviewIndex, setReviewIndex] = useState(0);
  const [testCommand, setTestCommand] = useState<string|null>(null);
  const [mutating, setMutating] = useState(false);
  const contextRef = useRef(context); contextRef.current = context;
  const report = reports[context.workspaceId];
  const busy = starting || !!report && BUSY.has(report.status);
  const remember = (r:InsightReport) => setReports(current => ({...current,[r.workspaceId]:r}));
  useEffect(() => {
    let gone=false; let off:(()=>void)|undefined;
    void listen<InsightReport>("ide-analysis", e => { if(!gone) remember(e.payload); }).then(fn => { if(gone)fn();else off=fn; });
    return()=>{gone=true;off?.();};
  },[]);
  useEffect(() => {
    if(!report || !BUSY.has(report.status))return;
    let gone=false;
    const poll=()=>void invoke<InsightReport>("ide_analysis_status",{id:report.id}).then(r=>{if(!gone)remember(r);}).catch(e=>{if(!gone)setError(String(e));});
    const timer=window.setInterval(poll,1400);poll();
    return()=>{gone=true;window.clearInterval(timer);};
  },[report?.id,report?.status]);
  useEffect(()=>{setSetupAction(null);setReviewIds([]);setTestCommand(null);setError("");},[context.workspaceId]);
  const request = (action:AnalysisAction) => { setError("");setQuestion("");setSetupAction(action);context.onShow(); };
  async function start() {
    if(!setupAction || busy)return;
    const c=contextRef.current;
    if(c.dirty){setError("Save all editor changes before analyzing disk files.");return;}
    setStarting(true);setError("");
    try {
      const r=await invoke<InsightReport>("ide_analysis_start",{request:{workspaceId:c.workspaceId,action:setupAction,scope:setupAction==="explain"?"file":scope,activeFile:c.activeFile??null,selectionStart:c.selectionStart??null,selectionEnd:c.selectionEnd??null,question}});
      remember(r);setSetupAction(null);
    }catch(e){setError(String(e));}finally{setStarting(false);}
  }
  async function implementSuggestions(findingIds:string[]) {
    const c=contextRef.current;
    const r=reports[c.workspaceId];
    if(!r){setError("Run an analysis first so JadX has findings to implement.");return;}
    if(c.dirty){setError("Save all editor changes before generating patches.");return;}
    if(c.disabled){setError(c.disabledReason??"Wait for the current agent task to finish.");return;}
    const goals=r.findings.filter(f=>findingIds.includes(f.id)&&f.suggestion.trim());
    if(!goals.length){setError("These findings do not contain implementable suggestions.");return;}
    const files=Array.from(new Set(goals.map(f=>f.path).filter((path):path is string=>Boolean(path))));
    const compact=(limit:number)=>[
      "Generate exact, reviewable code patches that implement every recommendation below. Analyze the supplied project source, preserve behavior and project conventions, and include edits in the refactor result. Do not apply changes; the user will review the diff.",
      "Recommendations:",
      ...goals.map((f,i)=>`${i+1}. ${f.title}${f.path?` (${f.path}${f.line?`:${f.line}`:""})`:""}: ${f.suggestion.slice(0,limit)}`),
    ].join("\n");
    let question=compact(640);
    if(question.length>48_000)question=compact(240);
    if(question.length>48_000){setError("This project has too many suggestions for one patch request. Implement them in smaller groups.");return;}
    setStarting(true);setError("");c.onShow();
    try{
      const next=await invoke<InsightReport>("ide_analysis_start",{request:{workspaceId:c.workspaceId,action:"refactor",scope:files.length===1?"file":"project",activeFile:files.length===1?files[0]:null,selectionStart:null,selectionEnd:null,question}});
      remember(next);
    }catch(e){setError(String(e));}finally{setStarting(false);}
  }
  async function cancel(){if(!report)return;try{await invoke("ide_analysis_cancel",{id:report.id});}catch(e){setError(String(e));}}
  async function reject(id:string){if(!report)return;try{await invoke("ide_patch_reject",{id:report.id,patchId:id});remember(await invoke<InsightReport>("ide_analysis_status",{id:report.id}));}catch(e){setError(String(e));}}
  async function apply(){
    if(!report||contextRef.current.dirty){setError("Save all editor changes before applying a patch.");return;}
    setMutating(true);setError("");
    try{remember(await invoke<InsightReport>("ide_patches_apply",{id:report.id,patchIds:reviewIds,approved:true}));setReviewIds([]);contextRef.current.onApplied();}catch(e){setError(String(e));}finally{setMutating(false);}
  }
  async function runTests(){if(!report||!testCommand)return;setMutating(true);setError("");try{await invoke("ide_tests_run",{id:report.id,commandId:testCommand,approved:true});remember(await invoke<InsightReport>("ide_analysis_status",{id:report.id}));setTestCommand(null);}catch(e){setError(String(e));}finally{setMutating(false);}}
  const review=(ids:string[])=>{setReviewIds(ids);setReviewIndex(0);setError("");};
  const files=report?.patches.filter(p=>reviewIds.includes(p.id)).flatMap(p=>p.files.map(f=>({...f,patchTitle:p.title})))??[];
  const file=files[reviewIndex];
  const test=report?.testCommands.find(t=>t.id===testCommand);
  const closeDialogs=()=>{if(!starting&&!mutating){setSetupAction(null);setReviewIds([]);setTestCommand(null);}};
  useEffect(()=>{if(!setupAction&&!reviewIds.length&&!testCommand)return;const escape=(e:KeyboardEvent)=>{if(e.key==="Escape")closeDialogs();};document.addEventListener("keydown",escape);return()=>document.removeEventListener("keydown",escape);},[setupAction,reviewIds.length,testCommand,starting,mutating]);
  const dialogs=(setupAction||files.length>0||test)&&createPortal(<div className="modal-backdrop intelligence-backdrop" onMouseDown={e=>{if(e.target===e.currentTarget)closeDialogs();}}>
    {setupAction?<section className="ai-action-dialog" role="dialog" aria-modal="true" aria-labelledby="ai-action-title">
      <header><div><Sparkles size={16}/><h2 id="ai-action-title">{ANALYSIS_ACTIONS.find(a=>a.id===setupAction)?.label}</h2></div><button autoFocus onClick={closeDialogs} aria-label="Close analysis setup"><X size={17}/></button></header>
      <p>{ANALYSIS_ACTIONS.find(a=>a.id===setupAction)?.description}</p>
      <label>Analyze<select aria-label="Analysis scope" value={setupAction==="explain"?"file":scope} disabled={setupAction==="explain"} onChange={e=>setScope(e.target.value as "file"|"project")}><option value="file" disabled={!context.activeFile}>Current file and related context</option><option value="project">Entire project</option></select></label>
      <div className="ai-context-summary"><strong>{context.workspaceName}</strong><span>{scope==="file"||setupAction==="explain"?context.activeFile??"Open a source file to continue":context.root}</span>{context.selectionStart&&context.selectionEnd&&<small>Selection: lines {context.selectionStart}–{context.selectionEnd}</small>}</div>
      <label>Focus or question <small>Optional · up to 48,000 characters</small><textarea value={question} onChange={e=>setQuestion(e.target.value)} maxLength={48000} rows={3} placeholder="What matters most for this review?"/></label>
      <p className="ai-sharing-note"><ShieldCheck size={14}/><span>{context.provider} receives eligible source context when you start. Secrets, hidden files, generated folders and binary files are excluded; detected credential lines are redacted. Large projects are analyzed in cancellable batches with visible coverage.</span></p>
      {context.mode&&<p className="ai-mode-note">Engineering Principles principles and contextual evaluation enabled.</p>}
      {(error||context.dirty||context.disabled)&&<p role="alert" className="ai-inline-error">{error||(context.dirty?"Save all editor changes first.":context.disabledReason??"Wait for the current agent task to finish.")}</p>}
      <footer><button onClick={closeDialogs}>Cancel</button><button className="ai-primary" onClick={()=>void start()} disabled={busy||context.dirty||context.disabled||((scope==="file"||setupAction==="explain")&&!context.activeFile)}>{starting?<LoaderCircle size={14} className="ide-spin"/>:<Sparkles size={14}/>}Start analysis</button></footer>
    </section>:file?<section className="ai-diff-dialog" role="dialog" aria-modal="true" aria-labelledby="ai-diff-title">
      <header><div><GitCompareArrows size={16}/><h2 id="ai-diff-title">Review proposed changes</h2></div><button autoFocus onClick={closeDialogs} disabled={mutating} aria-label="Close proposed changes"><X size={17}/></button></header>
      <nav aria-label="Proposed file changes">{files.map((f,i)=><button className={i===reviewIndex?"selected":""} key={`${f.path}-${i}`} onClick={()=>setReviewIndex(i)}>{f.path}{!f.expectedEtag&&<small>new</small>}</button>)}</nav>
      <div className="ai-diff-caption"><strong>{file.patchTitle}</strong><span>Original → Proposed · {reviewIndex+1}/{files.length}</span></div>
      <div className="ai-diff-editor"><DiffEditor original={file.before} modified={file.after} language={diffLanguage(file.path)} theme="vs-dark" options={{readOnly:true,originalEditable:false,renderSideBySide:true,automaticLayout:true,minimap:{enabled:false},scrollBeyondLastLine:false,wordWrap:"on",fontSize:12}}/></div>
      {error&&<p role="alert" className="ai-inline-error">{error}</p>}
      <footer><span>Exact source checks · Coordinated writes · Recovery snapshots</span>{reviewIds.length===1&&<button disabled={mutating} onClick={()=>{void reject(reviewIds[0]);setReviewIds([]);}}>Reject</button>}<button onClick={closeDialogs} disabled={mutating}>Cancel</button><button className="ai-primary" disabled={mutating||context.dirty} onClick={()=>void apply()}>{mutating?<LoaderCircle size={14} className="ide-spin"/>:<Check size={14}/>} {context.dirty?"Save editor changes first":reviewIds.length>1?`Approve & apply all (${reviewIds.length})`:"Accept & apply"}</button></footer>
    </section>:test?<section className="ai-action-dialog" role="dialog" aria-modal="true" aria-labelledby="ai-test-title"><header><h2 id="ai-test-title">Run project tests?</h2><button autoFocus onClick={closeDialogs} aria-label="Close test approval"><X size={17}/></button></header><p>Test scripts execute project code. Review the command and workspace before approving.</p><div className="ai-context-summary"><span>{context.root}</span><code>{test.command}</code></div><p>Uses JadX's Safe mode sandbox and existing environment. Network is blocked and the run is limited to 120 seconds. Missing dependencies are reported.</p>{error&&<p role="alert" className="ai-inline-error">{error}</p>}<footer><button onClick={closeDialogs}>Cancel</button><button className="ai-primary" onClick={()=>void runTests()} disabled={mutating||context.dirty}><FlaskConical size={14}/>Approve & run tests</button></footer></section>:null}
  </div>,document.body);
  return {report,busy,error,setError,request,cancel,reject,review,implementSuggestions,setTestCommand,dialogs};
}
export type InsightsController = ReturnType<typeof useIdeInsights>;

export function AiAnalysisToolbar({ai,disabled,onShow}:{ai:InsightsController;disabled:boolean;onShow:()=>void}){
  const [menuOpen,setMenuOpen]=useState(false);
  return <div className="ai-analysis-group" aria-label="AI analysis tools">
    {menuOpen&&<button className="ai-menu-backdrop" aria-label="Close analysis menu" tabIndex={-1} onClick={()=>setMenuOpen(false)}/>}
    <button className={`ai-analyze-button ${menuOpen?"open":""}`} disabled={ai.busy||disabled} onClick={()=>setMenuOpen(value=>!value)} aria-expanded={menuOpen} aria-haspopup="menu" title="Run an AI analysis">
      <Sparkles size={14}/><span>Analyze</span><ChevronDown size={13}/>
    </button>
    {menuOpen&&<div className="ai-analysis-menu" role="menu" aria-label="Analysis actions">
      {ANALYSIS_ACTIONS.map(({id,label,icon:Icon,description})=><button key={id} role="menuitem" onClick={()=>{setMenuOpen(false);ai.request(id);}}>
        <Icon size={15}/>
        <span><strong>{label}</strong><small>{description}</small></span>
      </button>)}
    </div>}
    {ai.busy&&<button onClick={()=>void ai.cancel()} title="Cancel analysis"><Square size={12}/><span>Stop</span></button>}
    <button onClick={onShow} title="Open the JadX Agent panel"><Search size={14}/><span>JadX Agent{ai.report?` · ${ai.report.findings.length}`:""}</span></button>
  </div>;
}

export function AiInsightsPanel({ai,onOpen,onFollowUp,onProfile}:{ai:InsightsController;onOpen:(path:string,line:number)=>void;onFollowUp:(prompt:string)=>void;onProfile:()=>void}){
  const [filter,setFilter]=useState("all");const [query,setQuery]=useState("");const [severity,setSeverity]=useState("all");
  const r=ai.report;
  const [visibleFindingLimit,setVisibleFindingLimit]=useState(100);
  useEffect(()=>setVisibleFindingLimit(100),[r?.id,filter,severity,query]);
  const categories=Array.from(new Set(r?.findings.map(f=>f.category)||[]));
  const findings=r?.findings.filter(f=>(filter==="all"||f.category===filter)&&(severity==="all"||f.severity===severity)&&`${f.title} ${f.path??""} ${f.explanation}`.toLowerCase().includes(query.toLowerCase()))??[];
  const visibleFindings=findings.slice(0,visibleFindingLimit);
  const pending=r?.patches.filter(p=>p.status==="pending")??[];
  const implementable=r?.findings.filter(f=>f.suggestion.trim()&&(!f.patchId||r.patches.find(p=>p.id===f.patchId)?.status!=="pending"))??[];
  return <div className="ai-insights-panel">
    <div className="ai-insights-heading"><div><Sparkles size={16}/><strong>AI Insights</strong></div>{r?.engineeringPrinciples&&<button className="ai-mode-indicator" onClick={onProfile}><Info size={13}/>Principles</button>}</div>
    {ai.error&&<div className="ai-inline-error" role="alert">{ai.error}<button onClick={()=>ai.setError("")} aria-label="Dismiss insights error"><X size={13}/></button></div>}
    {!r?<div className="ai-insights-empty"><Sparkles size={28}/><h3>A deeper view of your project</h3><p>Choose an analysis above. Findings connect to source lines, suggested patches and useful follow-up questions.</p><p>Refactors and tests are proposed for review before they reach your files.</p></div>:<>
      <section className="ai-analysis-progress" aria-live="polite"><div>{ai.busy?<LoaderCircle size={14} className="ide-spin"/>:<Check size={14}/>}<strong>{ANALYSIS_ACTIONS.find(a=>a.id===r.action)?.label??"Analysis"}</strong>{ai.busy&&<button onClick={()=>void ai.cancel()}><Square size={11}/>Stop</button>}</div><p>{r.message}</p>{r.coverage.totalBatches>0&&<progress max={r.coverage.totalBatches} value={r.coverage.completedBatches}/>}<small>{r.coverage.analyzedFiles} files analyzed · {r.coverage.indexedFiles} indexed · {r.coverage.excludedFiles} excluded</small><small>{r.model||r.provider}{r.coverage.usageReported?` · ${(r.coverage.inputTokens+r.coverage.outputTokens).toLocaleString()} reported tokens`:" · Usage will appear when reported"}</small><details><summary>Coverage and context</summary><p>{r.coverage.completedBatches}/{r.coverage.totalBatches} source batches · {r.coverage.contextTokens.toLocaleString()} token context. File scope includes related manifests and dependencies; project scope visits eligible source files. Dependencies and runtime behavior may require a focused follow-up. Findings are model recommendations, not a compiler or a security guarantee.</p><p>Provider usage is included in JadX's Usage report. No previous chat transcript or full research dossier is repeated in analysis requests.</p></details></section>
      {r.warnings.length>0&&<details className="ai-insight-warnings"><summary>{r.warnings.length} analysis notes</summary>{Array.from(new Set(r.warnings)).map(w=><p key={w}>{w}</p>)}</details>}
      <div className="ai-insight-controls"><label><Search size={13}/><input value={query} onChange={e=>setQuery(e.target.value)} placeholder="Filter findings" aria-label="Filter findings"/></label><div><select aria-label="Finding category" value={filter} onChange={e=>setFilter(e.target.value)}><option value="all">All categories</option>{categories.map(c=><option key={c}>{c}</option>)}</select><select aria-label="Finding severity" value={severity} onChange={e=>setSeverity(e.target.value)}><option value="all">All severities</option>{["critical","high","medium","low","info"].map(s=><option key={s}>{s}</option>)}</select></div></div>
      {pending.length>0&&<div className="ai-patch-batch"><span>{pending.length} proposed changes</span><button disabled={ai.busy} onClick={()=>ai.review(pending.map(p=>p.id))}><GitCompareArrows size={13}/>Review & apply all</button></div>}
      {implementable.length>0&&<div className="ai-patch-batch ai-implement-batch"><span>{implementable.length} suggestions without patches</span><button disabled={ai.busy} onClick={()=>void ai.implementSuggestions(implementable.map(f=>f.id))}><WandSparkles size={13}/>Implement Suggestions</button></div>}
      <div className="ai-findings">{categories.filter(c=>visibleFindings.some(f=>f.category===c)).map(category=><section key={category}><h3>{category} <span>{findings.filter(f=>f.category===category).length}</span></h3>{visibleFindings.filter(f=>f.category===category).map(f=>{const patch=r.patches.find(p=>p.id===f.patchId);return <article className="ai-finding" key={f.id}><div className="ai-finding-top"><span className={`ai-severity ${f.severity}`}>{f.severity}</span><span>Impact {f.impact}/5 · {f.effort||"unknown"} effort · {Math.round(f.confidence*100)}% confidence</span></div><h4>{f.title}</h4>{f.path&&<button className="ai-source-location" onClick={()=>onOpen(f.path!,f.line??1)}>{f.path}{f.line?`:${f.line}`:""}<ChevronRight size={12}/></button>}<p>{f.explanation}</p>{f.evidence&&<details><summary>Evidence</summary><p>{f.evidence}</p></details>}{f.suggestion&&<div className="ai-suggestion"><strong>Suggested approach</strong><p>{f.suggestion}</p></div>}{f.verification&&<details><summary>How to verify</summary><p>{f.verification}</p></details>}{f.evaluation&&<details className="ai-principle-evaluation"><summary>Why this fits the principles</summary>{Object.entries(f.evaluation).map(([k,v])=><p key={k}><strong>{k.replace(/([A-Z])/g," $1")}</strong>{v||"Not assessed"}</p>)}</details>}<div className="ai-finding-actions">{patch?.status==="pending"?<><button disabled={ai.busy} onClick={()=>ai.review([patch.id])}><GitCompareArrows size={13}/>Review patch</button><button disabled={ai.busy} onClick={()=>void ai.reject(patch.id)}>Reject</button></>:patch?<><span>{patch.status==="applied"?"Applied":patch.status==="stale"?"Source changed · rerun analysis": "Rejected"}</span>{patch.status!=="applied"&&f.suggestion.trim()&&<button disabled={ai.busy} onClick={()=>void ai.implementSuggestions([f.id])}><WandSparkles size={13}/>Try again</button>}</>:f.suggestion.trim()?<button disabled={ai.busy} onClick={()=>void ai.implementSuggestions([f.id])}><WandSparkles size={13}/>Implement suggestion</button>:null}<button disabled={ai.busy} onClick={()=>onFollowUp(`Discuss this IDE finding in the selected workspace. Inspect only relevant source as needed. Do not apply changes until the user requests them.\nFinding: ${f.title}\nLocation: ${f.path??"project"}:${f.line??1}\nEvidence: ${f.evidence}\nRecommendation: ${f.suggestion}\nQuestion: ${f.followUp||"What are the trade-offs and the smallest useful next step?"}`)}>Ask JadX</button></div>{f.followUp&&<small className="ai-followup-question">{f.followUp}</small>}</article>;})}</section>)}</div>
      {findings.length>visibleFindingLimit&&<button className="ai-show-more" onClick={()=>setVisibleFindingLimit(n=>n+100)}>Show next 100 findings · {findings.length-visibleFindingLimit} remaining</button>}
      {!findings.length&&<p className="ai-no-findings">{ai.busy?"Findings appear as each batch completes.":r.status==="completed"?"No findings match this view. Check the coverage and summary for analysis limits.":"No findings yet. Review the status above."}</p>}
      {r.summaries.length>0&&<details className="ai-analysis-summaries"><summary>Analysis summaries ({r.summaries.length})</summary>{r.summaries.map((s,i)=><p key={i}>{s}</p>)}</details>}
      {r.testCommands.length>0&&<section className="ai-run-tests"><h3>Project tests</h3>{r.testCommands.map(t=><button key={t.id} disabled={ai.busy} onClick={()=>ai.setTestCommand(t.id)}><FlaskConical size={14}/>{t.label}</button>)}</section>}
      {r.testOutput&&<details className="ai-test-output" open><summary>Test output</summary><pre>{r.testOutput}</pre></details>}
    </>}
  </div>;
}

function diffLanguage(path:string){const ext=path.split('.').pop();return ({ts:"typescript",tsx:"typescript",js:"javascript",jsx:"javascript",py:"python",rs:"rust",html:"html",css:"css",json:"json",go:"go",java:"java",swift:"swift"} as Record<string,string>)[ext??""]??"plaintext";}
