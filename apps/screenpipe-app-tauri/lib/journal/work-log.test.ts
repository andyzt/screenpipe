// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
import {describe,it,expect,vi} from "vitest";
vi.mock("@/lib/api",()=>({localFetch:vi.fn()}));
import {quarterRange,canApprove,reportMarkdown,type WorkNote,type WorkLogSnapshot} from "./work-log";
const note=(patch:Partial<WorkNote>={}):WorkNote=>({id:"1",revision:1,date:"2026-09-22",context:"work",project:"Release",outcome:"Reviewed rollback",status:"in_progress",role:"reviewer",approved:true,source_activity_id:45,source_activity_key:"private source",source_start_at:null,source_end_at:null,updated_at:"",...patch});
const snapshot=(entries:WorkNote[]):WorkLogSnapshot=>({entries,from:"2026-07-01",to:"2026-09-30",truncated:false});
const report=(s:WorkLogSnapshot)=>reportMarkdown(s,"2026-07-01","2026-09-30");
describe("reviewed work reports",()=>{
  it("crosses years and leap dates using local calendar quarters",()=>{
    expect(quarterRange(new Date(2026,0,10),-1)).toEqual({from:"2025-10-01",to:"2025-12-31"});
    expect(quarterRange(new Date(2024,1,29))).toEqual({from:"2024-01-01",to:"2024-03-31"});
  });
  it("requires work, a result and an explicit contributing role",()=>{
    for(const context of ["work","personal","mixed","unspecified"] as const)
      for(const role of ["owner","contributor","reviewer","observer","unknown"] as const)
        expect(canApprove(note({context,role}))).toBe(context==="work"&&["owner","contributor","reviewer"].includes(role));
    expect(canApprove(note({outcome:"  "}))).toBe(false);
  });
  it("exports only approved work, excluding even private project names and sources",()=>{
    const text=report(snapshot([note(),note({context:"personal",project:"secret1",outcome:"secret1"}),note({context:"mixed",outcome:"secret2"}),note({approved:false,outcome:"secret3"}),note({role:"observer",outcome:"secret4"}),note({date:"2026-10-01",outcome:"secret5"})]));
    expect(text).toContain("Reviewed rollback");expect(text).toContain("user-reported");
    expect(text).not.toMatch(/secret|private source/);
  });
  it("fails closed on truncation, mismatched dates or empty selection",()=>{
    expect(()=>report({...snapshot([note()]),truncated:true})).toThrow("incomplete");
    expect(()=>report({...snapshot([note()]),from:"2026-01-01"})).toThrow("incomplete");
    expect(()=>report(snapshot([]))).toThrow("No approved");
    expect(()=>reportMarkdown(snapshot([note()]),"2026-07-01","2026-09-30","Other")).toThrow("No approved");
  });
  it("keeps authored HTML, Markdown and newlines inert",()=>{
    const text=report(snapshot([note({outcome:'<script>x</script>\n# forged [link](https://evil.example)',project:'*test*'})]));
    expect(text).not.toContain("<script>");expect(text).not.toContain("\n# forged");expect(text).not.toContain("[link]");
    expect(text).toContain("&lt;script&gt;");
  });
});

describe("report sources", () => {
  it("appends only the snapshotted span of a linked card and never a title", async () => {
    const { reportMarkdown, quarterRange, sourceSpanSuffix } = await import("./work-log");
    const range = quarterRange(new Date(2026, 8, 22));
    const base = { revision: 1, context: "work" as const, project: "Alpha", status: "completed" as const, role: "owner" as const, approved: true,
      source_activity_key: "Auth *fix* [PR]", updated_at: "2026-09-22T10:00:00Z" };
    const linked = { ...base, id: "a", date: range.from, outcome: "Fixed auth", source_activity_id: 3,
      source_start_at: new Date(2026, 8, 22, 5, 10).toISOString(), source_end_at: new Date(2026, 8, 22, 5, 40).toISOString() };
    const unlinked = { ...base, id: "b", date: range.from, outcome: "Reviewed billing", source_activity_id: 99, source_start_at: null, source_end_at: null };
    const text = reportMarkdown({ truncated: false, from: range.from, to: range.to, entries: [linked, unlinked] }, range.from, range.to);
    expect(text).toContain("Fixed auth (journal card 2026-09-22 05:10–05:40)");
    expect(text).toMatch(/Reviewed billing\n/);
    expect(text).not.toContain("Auth");
    expect(sourceSpanSuffix({ source_start_at: "not a date", source_end_at: "x" })).toBe("");
  });
});
