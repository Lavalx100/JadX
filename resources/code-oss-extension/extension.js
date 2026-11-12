const vscode = require('vscode');
const http = require('http');
let timer;
exports.activate = function(context) {
  const base = process.env.JADX_IDE_BRIDGE_URL;
  const token = process.env.JADX_IDE_BRIDGE_TOKEN;
  if (!base || !token || !/^http:\/\/127\.0\.0\.1:\d+$/.test(base)) return;
  let busy = false, lastNavigation = '', lastMode;
  function request(path, body) {
    return new Promise((resolve,reject) => {
      const raw = body ? JSON.stringify(body) : '';
      const req = http.request(base + path, {method:body?'POST':'GET',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json','Content-Length':Buffer.byteLength(raw)}},res=>{
        let data='';res.on('data',c=>{data+=c;if(data.length>32000)req.destroy();});res.on('end',()=>{try{resolve(JSON.parse(data));}catch(e){reject(e);}});
      });
      req.setTimeout(2000,()=>req.destroy(new Error('Bridge timeout')));req.on('error',reject);req.end(raw);
    });
  }
  async function sync() {
    if(busy)return;busy=true;
    try {
      const editor=vscode.window.activeTextEditor;
      const root=vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
      const file=editor?.document.uri.scheme==='file'?editor.document.uri.fsPath:null;
      const relative=root&&file&&file.startsWith(root+'/')?file.slice(root.length+1):null;
      const selected=editor&&!editor.selection.isEmpty?editor.document.getText(editor.selection):'';
      await request('/context',{path:relative,selectionStart:editor?editor.selection.start.line+1:null,selectionEnd:editor?editor.selection.end.line+1:null,selectionText:selected.slice(0,12000),dirty:vscode.workspace.textDocuments.some(d=>d.isDirty)});
      const state=await request('/state');
      if(state.engineeringPrinciples!==lastMode){
        const config=vscode.workspace.getConfiguration('workbench');
        if(state.engineeringPrinciples){
          if(config.get('colorTheme')!=='JadX Engineering Principles')await context.globalState.update('previousTheme',config.get('colorTheme','Default Dark Modern'));
          await config.update('colorTheme','JadX Engineering Principles',vscode.ConfigurationTarget.Global);
        }else if(config.get('colorTheme')==='JadX Engineering Principles'){
          await config.update('colorTheme',context.globalState.get('previousTheme','Default Dark Modern'),vscode.ConfigurationTarget.Global);
        }
        lastMode=state.engineeringPrinciples;
      }
      if(state.navigation&&state.navigation.id!==lastNavigation&&root){
        const path=require('path');const target=path.resolve(root,state.navigation.path);
        if(target.startsWith(root+path.sep)){
          const doc=await vscode.workspace.openTextDocument(vscode.Uri.file(target));
          const position=new vscode.Position(Math.min(doc.lineCount-1,Math.max(0,state.navigation.line-1)),0);
          const shown=await vscode.window.showTextDocument(doc,{preview:false,selection:new vscode.Range(position,position)});
          shown.revealRange(new vscode.Range(position,position),vscode.TextEditorRevealType.InCenter);
        }
        lastNavigation=state.navigation.id;
      }
    }catch{ /* The desktop app may be restarting. Retry without exposing editor data. */ }
    finally{busy=false;}
  }
  timer=setInterval(sync,650);
  context.subscriptions.push({dispose(){clearInterval(timer);}},vscode.window.onDidChangeActiveTextEditor(sync),vscode.window.onDidChangeTextEditorSelection(sync),vscode.workspace.onDidSaveTextDocument(sync));
  sync();
};
exports.deactivate=function(){clearInterval(timer);};
