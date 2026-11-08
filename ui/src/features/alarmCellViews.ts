// THE VIEWS WHOSE TX STRIP DRAWS THE TRANSMITTER ALARM CELL (TxAlarmBanner: CockpitTxStrip / OperateQsoStrip
// `alarm`). App passes TopBar `alarmCell` from it: on these the top bar's lane may be cut, because every critical
// lane item is also shown beside Stop TX; everywhere else the lane keeps a critical item's flag and announces it
// (StatusLane `keepCritical`). A cockpit left off the list gets a floored lane and a second alert; one wrongly on
// it loses the floor where nothing else shows the alarm. alarmCellViews.test.tsx holds the list to the cockpits.
import type { View } from './registry'

export const ALARM_CELL_VIEWS: ReadonlySet<View> = new Set<View>(['operate', 'chat', 'phone', 'cw', 'rtty', 'psk', 'jtty', 'sstv', 'aprs', 'js8'])
