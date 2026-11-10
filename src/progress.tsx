import React, { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import './progress.css';

type UploadItem={name:string;phase:string;percent:number;error?:string;time?:string};
function Progress(){
 const [items,setItems]=useState<UploadItem[]>([]); const win=getCurrentWindow();
 useEffect(()=>{const un=listen<UploadItem>('upload-progress',e=>setItems(q=>[...q.filter(x=>x.name!==e.payload.name),e.payload])); const esc=(e:KeyboardEvent)=>{if(e.key==='Escape')void win.hide()};window.addEventListener('keydown',esc);return()=>{un.then(f=>f());window.removeEventListener('keydown',esc)}},[]);
 const done=items.filter(x=>x.percent===100).length;
 return <div className="queue"><div className="queue-title" onMouseDown={e=>{if(e.button===0 && !(e.target as HTMLElement).closest('button')){e.preventDefault();void win.startDragging().catch(()=>{})}}}><span>发送队列</span><span className="queue-summary">{done}/{items.length}</span><button onMouseDown={e=>e.stopPropagation()} onClick={()=>void win.hide()}>×</button></div><div className="queue-list">{items.length===0?<div className="queue-empty">暂无上传任务</div>:items.map(x=><div className="queue-item" key={x.name}><div className="item-head"><span className="item-name">{x.name}</span><span className="item-state">{x.phase} {x.percent}%{x.time&&<small> · {x.time}</small>}</span></div><div className="track"><i style={{width:`${x.percent}%`}}/></div>{x.error&&<small className="error">{x.error}</small>}</div>)}</div></div>
}
createRoot(document.getElementById('progress-root')!).render(<Progress/>);
