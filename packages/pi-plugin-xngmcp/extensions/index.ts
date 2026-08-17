import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const WEB_GUIDANCE = `
## Web research

Use web_search when the answer depends on current or external information that
is not already in the task context. Search with a focused query that names the
subject and the fact needed. Review the results, choose a relevant public URL,
and use web_fetch only for the page that can answer the question. Set
max_chars to a proportionate limit, such as 12000, before fetching. Do not
search for information already supplied by the user or available in the local
project.
`;

export default function xngmcpExtension(pi: ExtensionAPI) {
  pi.on("before_agent_start", (event) => {
    const activeTools = pi.getActiveTools();
    if (!activeTools.includes("web_search") || !activeTools.includes("web_fetch")) {
      return undefined;
    }

    return { systemPrompt: `${event.systemPrompt}${WEB_GUIDANCE}` };
  });
}
