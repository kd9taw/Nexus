import type { QueryPage } from '../application-query-protocol'
import fixture from './field-day.json'
export function fieldDayPage(): QueryPage {
  return {type:'applicationPage',requestId:crypto.randomUUID(),snapshotId:crypto.randomUUID(),collection:'fieldDay',offset:0,total:0,retained:0,nextCursor:null,ageMs:0,rows:[],meta:{capturedAgeMs:0,source:structuredClone(fixture)}}
}
