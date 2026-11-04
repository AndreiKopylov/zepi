// Reuse bundled pi's HTTP client dependency without modifying its dispatcher.
// One pooled connection per origin, HTTP/2 when supported, with proxy support.
import {EnvHttpProxyAgent, fetch} from 'undici';
const dispatcher=new EnvHttpProxyAgent({allowH2:true,connections:1});
export const modelFetch=(url:string,options:Parameters<typeof fetch>[1])=>fetch(url,{...options,dispatcher});
