import type {Theme} from './types';
export const themes:{id:Theme;name:string;description:string;background:string;color:string}[]=[
  {id:'forest',name:'森林浅色',description:'清爽绿色，适合日间阅读',background:'#f7f8f4',color:'#285e48'},
  {id:'paper',name:'暖纸米色',description:'柔和暖色，接近纸张观感',background:'#f6f0e5',color:'#795334'},
  {id:'night',name:'深海夜色',description:'深色背景，适合低光环境',background:'#151d28',color:'#9bcfbe'},
];
export function applyTheme(value:string,persist=false){
  const theme=themes.some(t=>t.id===value)?value:'forest';
  document.documentElement.dataset.theme=theme;
  if(persist)try{localStorage.setItem('course-workbench-theme',theme);}catch{/* Settings remain authoritative when browser storage is unavailable. */}
}
export function restoreTheme(){
  try{applyTheme(localStorage.getItem('course-workbench-theme')??'forest');}catch{applyTheme('forest');}
}
