import { MonitorApp } from './MonitorApp'
import { nativeSource } from './nativeSource'

export default function NativeMonitor() {
  return <MonitorApp source={nativeSource} />
}
