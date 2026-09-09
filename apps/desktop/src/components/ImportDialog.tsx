import { useEffect, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { FileUp, Link2, Globe2, ArrowRight, RefreshCw } from 'lucide-react';
import { api, errorMessage, isDesktop } from '../api';
import type { SourcePreview, SubtitleStatus } from '../types';
import { LOCAL_EXTENSIONS, singleLocalFile } from '../import-utils';
import { durationLabel, selectedScope, timeLabel } from '../utils';
import { Modal, Spinner } from './Common';
import './ImportDialog.css';

type Entry = 'bilibili' | 'web' | 'local';
const statusLabel: Record<SubtitleStatus, string> = {
  available: '可提取字幕', absent: '没有可读字幕', loginRequired: '字幕需要登录',
  failed: '字幕请求失败', unchecked: '处理前检查字幕',
};
const entries = [
  { id: 'bilibili' as const, label: 'B 站链接', Icon: Link2 },
  { id: 'web' as const, label: '其他视频网站', Icon: Globe2 },
  { id: 'local' as const, label: '本地文件', Icon: FileUp },
];

export default function ImportDialog({ onClose, onImported }: { onClose: () => void; onImported: () => Promise<void> }) {
  const [entry, setEntry] = useState<Entry>('bilibili');
  const [source, setSource] = useState('');
  const [preview, setPreview] = useState<SourcePreview | null>(null);
  const [pages, setPages] = useState(new Set<number>());
  const [mode, setMode] = useState('auto');
  const [busy, setBusy] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState('');
  const requestId = useRef(0);
  const submittingRef = useRef(false);
  const dropHandler = useRef<(paths: string[]) => void>(() => {});

  function changeSource(value: string) {
    requestId.current++;
    setSource(value); setPreview(null); setPages(new Set()); setError(''); setBusy('');
  }
  function changeEntry(value: Entry) {
    if (submittingRef.current) return;
    setEntry(value); changeSource(''); setMode('auto'); setDragging(false);
  }
  async function analyze(value = source, kind = entry) {
    if (!value.trim() || submittingRef.current) return;
    const id = ++requestId.current;
    setBusy('正在读取资料信息'); setError('');
    try {
      const input = kind === 'local' ? singleLocalFile([value]) : value.trim();
      const result = await api.probeSource(input);
      if (requestId.current !== id) return;
      setPreview(result); setPages(new Set(result.parts.length ? [result.parts[0].page] : []));
      if (result.sourceKind === 'subtitle' && mode === 'transcribe') setMode('auto');
    } catch (e) { if (requestId.current === id) setError(errorMessage(e)); }
    finally { if (requestId.current === id) setBusy(''); }
  }
  async function acceptFile(paths: string[]) {
    if (submittingRef.current) return;
    try {
      const path = singleLocalFile(paths);
      setEntry('local'); changeSource(path); setMode('auto');
      await analyze(path, 'local');
    } catch (e) { setError(errorMessage(e)); }
  }
  dropHandler.current = paths => { void acceptFile(paths); };
  useEffect(() => {
    if (!isDesktop()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview().onDragDropEvent(event => {
      if (submittingRef.current) return;
      if (event.payload.type === 'enter' || event.payload.type === 'over') setDragging(true);
      else {
        setDragging(false);
        if (event.payload.type === 'drop') dropHandler.current(event.payload.paths);
      }
    }).then(stop => { if (disposed) stop(); else unlisten = stop; })
      .catch(e => { if (!disposed) setError(`拖入文件暂不可用，请点击选择文件。${errorMessage(e)}`); });
    return () => { disposed = true; requestId.current++; unlisten?.(); };
  }, []);
  async function chooseFile() {
    try {
      const path = await open({ multiple: false, filters: [{ name: '音视频与字幕', extensions: [...LOCAL_EXTENSIONS] }] });
      if (typeof path === 'string') await acceptFile([path]);
    } catch (e) { setError(errorMessage(e)); }
  }
  const scope = selectedScope(preview?.parts ?? [], pages);
  async function submit() {
    if (!preview || !scope.pages.length || submittingRef.current) return;
    submittingRef.current = true; setSubmitting(true); setBusy('正在添加任务'); setError('');
    try { await api.createJobs(preview.source, scope.pages, mode); await onImported(); onClose(); }
    catch (e) { setError(errorMessage(e)); }
    finally { submittingRef.current = false; setSubmitting(false); setBusy(''); }
  }
  async function checkPart(page: number) {
    if (!preview) return;
    const id = requestId.current;
    setBusy(`正在检查 P${page} 的字幕`); setError('');
    try {
      const part = await api.probePart(preview.source, page);
      if (id === requestId.current) setPreview(current => current ? { ...current, parts: current.parts.map(p => p.page === page ? part : p) } : current);
    } catch (e) { if (id === requestId.current) setError(errorMessage(e)); }
    finally { if (id === requestId.current) setBusy(''); }
  }
  function close() { if (!submittingRef.current) { requestId.current++; onClose(); } }
  const label = entry === 'bilibili' ? 'B 站视频链接或 BV 号' : entry === 'web' ? '视频网页或音视频直链' : '本地文件路径';
  const placeholder = entry === 'bilibili' ? 'https://www.bilibili.com/video/BV…' : entry === 'web' ? 'https://…' : 'D:\\课程\\第一课.mp4';

  return <Modal title="导入课程" wide onClose={close}>
    <div className="modal-content import-content">
      <div className="import-entry-tabs" role="tablist" aria-label="导入来源">
        {entries.map(({ id, label: title, Icon }) => <button key={id} role="tab" aria-selected={entry === id} disabled={submitting} className={entry === id ? 'active' : ''} onClick={() => changeEntry(id)}><Icon size={18} />{title}</button>)}
      </div>
      {entry === 'web' && <p className="hint">读取单个视频页面或音视频直链。网站登录要求和解析支持情况会在读取时显示；播放列表与直播暂不导入。</p>}
      {(entry === 'local' || dragging) && <button className={`local-file-drop ${dragging ? 'dragging' : ''}`} disabled={submitting} onClick={() => void chooseFile()}>
        <FileUp size={27} /><strong>{dragging ? '松开以读取文件' : '选择本地文件，或将文件拖到这里'}</strong>
        <span>音频、视频、SRT / VTT 字幕 · 一次一个文件 · 资料保存在本机</span>
      </button>}
      <label className="field">{label}<div className="input-action"><Link2 size={18} />
        <input aria-label={label} autoFocus disabled={submitting} value={source} onChange={e => changeSource(e.target.value)} placeholder={placeholder} onKeyDown={e => { if (e.key === 'Enter' && !busy) void analyze(); }} />
        <button className="primary" disabled={!!busy || !source.trim()} onClick={() => void analyze()}>读取<ArrowRight size={15} /></button>
      </div></label>
      {entry !== 'local' && <button className="text-button" disabled={submitting} onClick={() => changeEntry('local')}><FileUp size={15} />已有下载文件？从本地导入</button>}
      {error && <div className="error-banner" role="alert">{error}</div>}
      {preview && <section className="import-preview">
        <div className="section-heading"><div><span className="eyebrow">处理范围</span><h3>{preview.title}</h3></div><span className="muted">{preview.parts.length} {preview.sourceKind === 'bilibili' ? '个分 P' : '份资料'}</span></div>
        {preview.parts.length > 1 && <div className="selection-tools"><button className="text-button" disabled={submitting} onClick={() => setPages(new Set(preview.parts.map(p => p.page)))}>选择全部 {preview.parts.length} 个分 P</button><button className="text-button" disabled={submitting} onClick={() => setPages(new Set())}>清空选择</button></div>}
        <div className="parts-list">{preview.parts.map(part => <div className={`part-row ${pages.has(part.page) ? 'selected' : ''}`} key={part.page}>
          <label><input type="checkbox" disabled={submitting} aria-label={`选择 ${part.title}`} checked={pages.has(part.page)} onChange={() => setPages(current => { const next = new Set(current); next.has(part.page) ? next.delete(part.page) : next.add(part.page); return next; })} />{preview.sourceKind === 'bilibili' && <span className="part-number">P{part.page}</span>}<span className="part-name">{part.title}</span></label>
          <span className="mono muted">{part.durationMs ? timeLabel(part.durationMs) : '时长待检测'}</span><span className={`status status-${part.subtitleStatus}`}>{statusLabel[part.subtitleStatus]}</span>
          {preview.sourceKind === 'bilibili' && <button className="icon-button" aria-label={`检查 P${part.page} 字幕`} disabled={!!busy} onClick={() => void checkPart(part.page)}><RefreshCw size={14} /></button>}
        </div>)}</div>
        {preview.warnings.map((warning, i) => <p className="hint" key={i}>{warning}</p>)}
      </section>}
      <fieldset className="mode-options" disabled={submitting}><legend>处理方式</legend>{[
        ['auto', '字幕优先', preview?.sourceKind === 'localMedia' ? '读取本地音视频，提取音频并转写。' : '可读取字幕时直接提取；否则获取音频并转写。'],
        ['subtitlesOnly', '仅提取字幕', '不获取媒体。需要登录或请求失败时显示具体原因。'],
        ['transcribe', '重新转写音频', '使用当前识别配置，生成独立文字版本。'],
      ].map(([value, title, description]) => <label className={mode === value ? 'chosen' : ''} key={value}><input type="radio" name="import-mode" checked={mode === value} disabled={preview?.sourceKind === 'subtitle' && value === 'transcribe'} onChange={() => setMode(value)} /><span><strong>{title}</strong><small>{description}</small></span></label>)}</fieldset>
    </div>
    <footer><div>{busy ? <Spinner label={busy} /> : <><strong>已选 {scope.pages.length} 项</strong><span className="muted"> · {durationLabel(scope.durationMs)}</span></>}</div><div className="actions"><button disabled={submitting} onClick={close}>取消</button><button className="primary" disabled={!!busy || !preview || !scope.pages.length} onClick={() => void submit()}>开始处理<ArrowRight size={16} /></button></div></footer>
  </Modal>;
}
