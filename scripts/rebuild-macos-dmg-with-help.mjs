import fs from 'fs'
import path from 'path'
import { execFileSync } from 'child_process'

const cwd = process.cwd()
const target = process.argv[2]
const releaseDir = target ? path.join(cwd, 'target', target, 'release') : path.join(cwd, 'target', 'release')
const appName = 'kaif vpnchik.app'
const dmgDir = path.join(releaseDir, 'bundle', 'dmg')
const appDir = path.join(releaseDir, 'bundle', 'macos')
const scriptPath = path.join(dmgDir, 'bundle_dmg.sh')
const helpPath = path.join(cwd, 'src-tauri', 'packages', 'macos', 'help.txt')
const backgroundPath = path.join(cwd, 'src-tauri', 'images', 'background.png')

if (!fs.existsSync(scriptPath)) {
  throw new Error(`DMG helper script not found: ${scriptPath}`)
}

if (!fs.existsSync(path.join(appDir, appName))) {
  throw new Error(`macOS app bundle not found: ${path.join(appDir, appName)}`)
}

if (!fs.existsSync(helpPath)) {
  throw new Error(`help.txt not found: ${helpPath}`)
}

const dmgName = fs.readdirSync(dmgDir).find((name) => name.endsWith('.dmg') && name.startsWith('kaif vpnchik_'))
if (!dmgName) {
  throw new Error(`kaif vpnchik DMG not found in ${dmgDir}`)
}

const dmgPath = path.join(dmgDir, dmgName)
fs.rmSync(dmgPath, { force: true })

execFileSync(
  scriptPath,
  [
    '--volname',
    'kaif vpnchik',
    '--volicon',
    path.join(dmgDir, 'icon.icns'),
    '--background',
    backgroundPath,
    '--window-pos',
    '200',
    '180',
    '--window-size',
    '660',
    '400',
    '--icon',
    appName,
    '170',
    '170',
    '--app-drop-link',
    '490',
    '170',
    '--add-file',
    'help.txt',
    helpPath,
    '330',
    '300',
    dmgPath,
    appDir,
  ],
  { stdio: 'inherit' },
)
