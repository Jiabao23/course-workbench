import { useCallback,useEffect,useRef,useState,type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { LoaderCircle,X,BookOpen } from 'lucide-react';

export function Spinner({label='处理中'}:{label?:string}) {return <span className="inline-spinner" role="status"><LoaderCircle size={16} className="spin"/>{label}</span>;}
export function Empty({title,children,action}:{title:string;children?:ReactNode;action?:ReactNode}) {return <div className="empty"><BookOpen size={34} strokeWidth={1.3}/><h2>{title}</h2><p>{children}</p>{action}</div>;}
export function Modal({title,children,onClose,wide=false}:{title:string;children:ReactNode;onClose:()=>void;wide?:boolean}) {
  const container=useRef<HTMLDivElement>(null);const close=useRef(onClose);close.current=onClose;
  useEffect(()=>{
    const previous=document.activeElement as HTMLElement|null;
    const root=container.current!;root.focus();
    const onKey=(event:KeyboardEvent)=>{
      if(event.key==='Escape'){event.preventDefault();close.current();}
      if(event.key==='Tab'){
        const focusable=Array.from(root.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), a[href], [tabindex="0"]')).filter(el=>el.offsetParent!==null);
        const first=focusable[0],last=focusable[focusable.length-1];
        if(!first){event.preventDefault();return;}
        if(event.shiftKey&&(document.activeElement===first||document.activeElement===root)){event.preventDefault();last.focus();}
        else if(!event.shiftKey&&(document.activeElement===last||document.activeElement===root)){event.preventDefault();first.focus();}
      }
    };
    root.addEventListener('keydown',onKey);return()=>{root.removeEventListener('keydown',onKey);previous?.focus();};
  },[]);
  return createPortal(<div className="modal-backdrop" onMouseDown={e=>{if(e.target===e.currentTarget)onClose();}}><div ref={container} tabIndex={-1} role="dialog" aria-modal="true" aria-label={title} className={`modal ${wide?'wide':''}`}><header><h2>{title}</h2><button className="icon-button" aria-label="关闭" onClick={onClose}><X size={20}/></button></header>{children}</div></div>,document.body);
}
export function useConfirm() {
  const [request,setRequest]=useState<{message:string;resolve:(value:boolean)=>void}|null>(null);
  const confirm=useCallback((message:string)=>new Promise<boolean>(resolve=>setRequest({message,resolve})),[]);
  const finish=(value:boolean)=>{request?.resolve(value);setRequest(null);};
  const dialog=request?<Modal title="确认操作" onClose={()=>finish(false)}><div className="modal-content"><p>{request.message}</p></div><footer><button onClick={()=>finish(false)}>返回</button><button className="primary" onClick={()=>finish(true)}>继续</button></footer></Modal>:null;
  return {confirm,dialog};
}
