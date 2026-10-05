// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
import React from "react";
import {describe,it,expect,vi,beforeEach,afterEach} from "vitest";
import {render,screen,fireEvent,waitFor,within} from "@testing-library/react";
const api=vi.hoisted(()=>({fetch:vi.fn(),save:vi.fn(),remove:vi.fn(),copy:vi.fn()}));
vi.mock("@/lib/utils/tauri",()=>({commands:{copyTextToClipboard:api.copy}}));
vi.mock("@/lib/journal/work-log",async()=>({...await vi.importActual<typeof import("@/lib/journal/work-log")>("@/lib/journal/work-log"),fetchWorkLog:api.fetch,saveWorkNote:api.save,deleteWorkNote:api.remove}));
import {WorkLogDialog} from "./work-log-dialog";
import {quarterRange} from "@/lib/journal/work-log";
const range=quarterRange();
const entry={id:"fixture",revision:1,date:range.from,context:"work",project:"Release",outcome:"Reviewed rollback",status:"in_progress",role:"reviewer",approved:true,source_activity_id:null};
beforeEach(()=>{vi.clearAllMocks();api.fetch.mockResolvedValue({entries:[entry],truncated:false,...range});api.copy.mockResolvedValue({status:"ok",data:null});api.save.mockResolvedValue({entry});Object.defineProperty(window,"__TAURI_INTERNALS__",{configurable:true,value:{}})});
afterEach(()=>{delete (window as any).__TAURI_INTERNALS__});
async function open(){render(<WorkLogDialog/>);fireEvent.click(screen.getByRole("button",{name:"Work report",exact:true}));await screen.findByText("Reviewed rollback")}
describe("work note review and export",()=>{
  it("copies fresh data through the native clipboard after delayed network response",async()=>{
    await open();let resolve!:(value:unknown)=>void;api.fetch.mockImplementationOnce(()=>new Promise(r=>resolve=r));
    fireEvent.click(screen.getByRole("button",{name:"Copy approved work"}));expect(api.copy).not.toHaveBeenCalled();
    resolve({entries:[{...entry,outcome:"Newly reviewed result"}],truncated:false,...range});
    await waitFor(()=>expect(api.copy).toHaveBeenCalledTimes(1));
    expect(api.copy.mock.calls[0][0]).toContain("Newly reviewed result");expect(api.copy.mock.calls[0][0]).not.toContain("Reviewed rollback");
  });
  it("does not export cached data on failed refresh or truncation",async()=>{
    await open();api.fetch.mockRejectedValueOnce(new Error("Offline"));fireEvent.click(screen.getByRole("button",{name:"Copy approved work"}));
    // A network failure shows the action's own localized message, not raw error text.
    await screen.findByText("Could not create report");expect(screen.queryByText("Offline")).not.toBeInTheDocument();expect(api.copy).not.toHaveBeenCalled();
    api.fetch.mockResolvedValueOnce({entries:[entry],truncated:true,...range});fireEvent.click(screen.getByRole("button",{name:"Copy approved work"}));
    await screen.findByText(/Report is incomplete/);expect(api.copy).not.toHaveBeenCalled();
  });
  it("requires reapproval after any authored field changes",async()=>{
    await open();fireEvent.click(screen.getByRole("button",{name:"Edit note",exact:true}));
    const approve=screen.getByRole("checkbox",{name:"Approve for work reports"});expect(approve).not.toBeChecked();
    fireEvent.click(approve);expect(approve).toBeChecked();fireEvent.change(screen.getByLabelText("Note project"),{target:{value:"Personal side project"}});expect(approve).not.toBeChecked();
    fireEvent.change(screen.getByLabelText("Note context",{exact:true}),{target:{value:"mixed"}});expect(approve).toBeDisabled();
    fireEvent.click(screen.getByRole("button",{name:"Save note",exact:true}));await waitFor(()=>expect(api.save).toHaveBeenCalled());
    expect(api.save.mock.calls[0][0]).toMatchObject({context:"mixed",approve:false,expected_revision:1});
  });
  it("reports a native clipboard error without claiming success",async()=>{
    await open();api.copy.mockResolvedValueOnce({status:"error",error:"denied"});fireEvent.click(screen.getByRole("button",{name:"Copy approved work"}));
    await screen.findByText(/Clipboard copy failed/);expect(screen.queryByText(/report copied/)).not.toBeInTheDocument();
  });
  it("clears a pending deletion before starting a new draft",async()=>{
    await open();fireEvent.click(screen.getByRole("button",{name:"Delete note",exact:true}));
    expect(screen.getByRole("button",{name:"Confirm delete note",exact:true})).toBeEnabled();
    fireEvent.click(screen.getByRole("button",{name:"New note",exact:true}));
    fireEvent.change(screen.getByLabelText("Contribution and result"),{target:{value:"Unsaved contribution"}});
    expect(screen.queryByRole("button",{name:"Confirm delete note",exact:true})).not.toBeInTheDocument();
    expect(screen.getByRole("button",{name:"Delete note",exact:true})).toBeDisabled();
    expect(screen.getByLabelText("Contribution and result")).toHaveValue("Unsaved contribution");
    expect(api.remove).not.toHaveBeenCalled();
  });
});
const journal=vi.hoisted(()=>({day:vi.fn(),recap:vi.fn()}));
vi.mock("@/lib/journal/api",()=>({fetchJournalDay:journal.day,fetchRecap:journal.recap}));
describe("journal suggestions",()=>{
  it("reads the window once, in parallel, drafts a linked note without copying card text, and does not re-read after saving",async()=>{
    const {makeJournalDay,makeActivityCard}=await import("@/lib/journal/fixtures");
    journal.day.mockImplementation(async(date:string)=>makeJournalDay({date,activities:[makeActivityCard({id:31,title:"Auth fix",active_minutes:35,start_at:`${date}T08:00:00Z`,end_at:`${date}T09:00:00Z`})]}));
    await open();fireEvent.click(screen.getByRole("button",{name:"Suggest from last 7 days"}));
    const list=await screen.findByRole("list",{name:"Contribution candidates"});
    expect(journal.day).toHaveBeenCalledTimes(7);expect(journal.recap).not.toHaveBeenCalled();
    expect(within(list).getAllByRole("button",{name:"Draft note"})).toHaveLength(7);
    fireEvent.click(within(list).getAllByRole("button",{name:"Draft note"})[0]);
    expect(screen.getByTestId("work-log-source-hint")).toHaveTextContent("Journal card (model-titled): “Auth fix”");
    expect(screen.getByLabelText("Contribution and result")).toHaveValue("");
    fireEvent.change(screen.getByLabelText("Contribution and result"),{target:{value:"Found the rollback gap"}});
    fireEvent.click(screen.getByRole("button",{name:"Save note",exact:true}));
    await waitFor(()=>expect(api.save).toHaveBeenCalled());
    expect(api.save.mock.calls[0][0]).toMatchObject({source_activity_id:31,outcome:"Found the rollback gap",approve:false});
    await screen.findByRole("list",{name:"Contribution candidates"});
    expect(journal.day).toHaveBeenCalledTimes(7);
  });
  it("reads recaps in parallel only for days that have one and marks noted cards",async()=>{
    const {makeJournalDay,makeActivityCard,makeRecap}=await import("@/lib/journal/fixtures");
    journal.day.mockImplementation(async(date:string)=>makeJournalDay({date,recap:date.endsWith("2")?{status:"ready",generated_at:date}:{status:"none",generated_at:null},activities:[makeActivityCard({id:Number(date.slice(-2)),title:`Task ${date}`,active_minutes:20})]}));
    journal.recap.mockImplementation(async(date:string)=>makeRecap({date,done:["Helped QA"]}));
    api.fetch.mockResolvedValue({entries:[{...entry,source_activity_id:22}],truncated:false,...range});
    render(<WorkLogDialog/>);fireEvent.click(screen.getByRole("button",{name:"Work report",exact:true}));await screen.findByText("Reviewed rollback");
    fireEvent.click(screen.getByRole("button",{name:"Suggest from last 14 days"}));
    const list=await screen.findByRole("list",{name:"Contribution candidates"});
    expect(journal.day).toHaveBeenCalledTimes(14);
    const recapDays=journal.day.mock.calls.map(c=>c[0] as string).filter(d=>d.endsWith("2"));
    expect(journal.recap).toHaveBeenCalledTimes(recapDays.length);
    expect(within(list).getAllByText(/recap \(model-written\)/)).toHaveLength(recapDays.length);
    expect(within(list).getAllByRole("button",{name:"Add another note"}).length).toBeGreaterThanOrEqual(1);
  });
  it("exports the snapshotted span of a linked note without any card title",async()=>{
    api.fetch.mockResolvedValue({entries:[{...entry,id:"linked",source_activity_id:5,source_start_at:new Date(2026,8,22,5,0).toISOString(),source_end_at:new Date(2026,8,22,6,0).toISOString(),outcome:"Linked result"},{...entry,id:"plain",outcome:"Plain result"}],truncated:false,...range});
    render(<WorkLogDialog/>);fireEvent.click(screen.getByRole("button",{name:"Work report",exact:true}));await screen.findByText("Linked result");
    fireEvent.click(screen.getByRole("button",{name:"Preview approved work"}));
    const preview=await screen.findByLabelText("Work report preview");
    expect(preview).toHaveTextContent("Linked result (journal card 2026-09-22 05:00–06:00)");
    expect(preview.textContent).toMatch(/Plain result\n/);
    expect(journal.day).not.toHaveBeenCalled();
  });
});
