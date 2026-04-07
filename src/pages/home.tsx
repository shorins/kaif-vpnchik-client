import {
  CloudUploadRounded,
  PowerSettingsNewRounded,
  RocketLaunchRounded,
} from '@mui/icons-material'
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  FormControlLabel,
  Stack,
  Switch as MuiSwitch,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import type { ChangeEvent } from 'react'
import { useMemo, useRef, useState } from 'react'

import { BasePage } from '@/components/base'
import { useProfiles } from '@/hooks/use-profiles'
import { useServiceInstaller } from '@/hooks/use-service-installer'
import { useSystemState } from '@/hooks/use-system-state'
import { useVerge } from '@/hooks/use-verge'
import {
  isAdmin,
  isServiceAvailable,
  importWireguardConf,
  setKaifVpnEnabled,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

const MANAGED_PROFILE_NAME = 'kaif vpnchik'

const readFileAsText = (file: File) =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = (event) => resolve(String(event.target?.result ?? ''))
    reader.onerror = reject
    reader.readAsText(file)
  })

const HomePage = () => {
  const inputRef = useRef<HTMLInputElement>(null)
  const { current, mutateProfiles } = useProfiles()
  const { verge, patchVerge } = useVerge()
  const { isTunModeAvailable, mutateSystemState } = useSystemState()
  const { installServiceAndRestartCore } = useServiceInstaller()
  const [lastImport, setLastImport] = useState<IWireGuardImportResult | null>(
    null,
  )
  const [dragActive, setDragActive] = useState(false)

  const hasManagedProfile =
    current?.name === MANAGED_PROFILE_NAME || !!lastImport
  const tunEnabled = verge?.enable_tun_mode ?? false
  const autoLaunchEnabled = verge?.enable_auto_launch ?? true

  const endpointLabel = useMemo(() => {
    if (lastImport) return `${lastImport.server}:${lastImport.port}`
    if (current?.name === MANAGED_PROFILE_NAME) return 'Конфиг импортирован'
    return 'Конфиг ещё не импортирован'
  }, [current?.name, lastImport])

  const handleImportFile = useLockFn(async (file: File) => {
    if (!file.name.toLowerCase().endsWith('.conf')) {
      showNotice.error('Выберите WireGuard .conf файл')
      return
    }

    try {
      const fileData = await readFileAsText(file)
      const result = await importWireguardConf(fileData, file.name)
      setLastImport(result)
      await mutateProfiles()
      showNotice.success(
        result.replacedExisting
          ? 'WireGuard конфиг обновлён'
          : 'WireGuard конфиг импортирован',
      )
    } catch (err) {
      showNotice.error(err)
    }
  })

  const handleFileInput = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    if (file) void handleImportFile(file)
    event.target.value = ''
  }

  const handleToggleVpn = useLockFn(async () => {
    if (!hasManagedProfile) {
      showNotice.error('Сначала импортируйте WireGuard .conf файл')
      return
    }

    try {
      const next = !tunEnabled

      if (next) {
        const [adminMode, serviceOk] = await Promise.all([
          isAdmin(),
          isServiceAvailable(),
        ])

        if (!adminMode && !serviceOk) {
          showNotice.info('Для TUN нужно установить системный сервис')
          await installServiceAndRestartCore()

          const refreshed = await mutateSystemState()
          const serviceInstalled =
            refreshed.data?.isServiceOk || (await isServiceAvailable())
          if (!serviceInstalled) {
            showNotice.error(
              'Сервис не установлен. Подключение VPN не включено.',
            )
            return
          }
        }
      }

      await setKaifVpnEnabled(next)
      await patchVerge({ enable_system_proxy: false, enable_tun_mode: next })
      showNotice.success(next ? 'VPN подключён' : 'VPN отключён')
    } catch (err) {
      showNotice.error(err)
    }
  })

  const handleAutoLaunch = useLockFn(async (_: unknown, checked: boolean) => {
    try {
      await patchVerge({
        enable_auto_launch: checked,
        enable_silent_start: checked,
      })
      showNotice.success(checked ? 'Автозапуск включён' : 'Автозапуск выключен')
    } catch (err) {
      showNotice.error(err)
    }
  })

  return (
    <BasePage
      title="kaif vpnchik"
      contentStyle={{
        minHeight: '100%',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 24,
      }}
    >
      <Card sx={{ width: '100%', maxWidth: 560, borderRadius: 4 }}>
        <CardContent>
          <Stack spacing={3}>
            <Box sx={{ textAlign: 'center' }}>
              <RocketLaunchRounded color="primary" sx={{ fontSize: 54 }} />
              <Typography variant="h4" sx={{ fontWeight: 700, mt: 1 }}>
                kaif vpnchik
              </Typography>
              <Typography color="text.secondary" sx={{ mt: 1 }}>
                Импортируйте WireGuard .conf, а приложение само подготовит
                Clash/Mihomo профиль с DIRECT для российских доменов.
              </Typography>
            </Box>

            <Box
              onDragOver={(event) => {
                event.preventDefault()
                setDragActive(true)
              }}
              onDragLeave={() => setDragActive(false)}
              onDrop={(event) => {
                event.preventDefault()
                setDragActive(false)
                const file = event.dataTransfer.files?.[0]
                if (file) void handleImportFile(file)
              }}
              sx={{
                border: '2px dashed',
                borderColor: dragActive ? 'primary.main' : 'divider',
                borderRadius: 3,
                p: 3,
                textAlign: 'center',
                bgcolor: dragActive ? 'action.hover' : 'transparent',
              }}
            >
              <CloudUploadRounded color="primary" sx={{ fontSize: 42 }} />
              <Typography sx={{ mt: 1, fontWeight: 600 }}>
                Перетащите .conf файл сюда
              </Typography>
              <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
                или выберите WireGuard конфиг вручную
              </Typography>
              <Button
                variant="outlined"
                onClick={() => inputRef.current?.click()}
              >
                Выбрать .conf
              </Button>
              <input
                ref={inputRef}
                type="file"
                accept=".conf"
                hidden
                onChange={handleFileInput}
              />
            </Box>

            <Alert severity={hasManagedProfile ? 'success' : 'info'}>
              {endpointLabel}
            </Alert>

            {!isTunModeAvailable && (
              <Alert severity="warning">
                Для TUN режима нужны права администратора или системный сервис.
                При подключении приложение предложит установить сервис.
              </Alert>
            )}

            <Button
              size="large"
              variant="contained"
              color={tunEnabled ? 'error' : 'primary'}
              startIcon={<PowerSettingsNewRounded />}
              disabled={!hasManagedProfile}
              onClick={handleToggleVpn}
              sx={{ py: 1.4, borderRadius: 3, fontWeight: 700 }}
            >
              {tunEnabled ? 'Отключить VPN' : 'Подключить VPN'}
            </Button>

            <FormControlLabel
              control={
                <MuiSwitch
                  checked={autoLaunchEnabled}
                  onChange={handleAutoLaunch}
                />
              }
              label="Запускать при старте системы"
            />

            <Typography variant="caption" color="text.secondary">
              Системный прокси не используется. При закрытии окна приложение
              остаётся в трее, где VPN можно быстро включить или выключить.
            </Typography>
          </Stack>
        </CardContent>
      </Card>
    </BasePage>
  )
}

export default HomePage
