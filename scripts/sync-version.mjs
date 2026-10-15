#!/usr/bin/env node
/**
 * 把一个版本号同步写进所有声明它的地方。
 *
 * 版本散在五处，其中 `src/views/SettingsView.vue` 里那个 APP_VERSION 是纯手写
 * 常量——漏掉的话，设置页「关于与更新」显示的版本会和 exe 文件属性对不上，
 * 同一个应用出现两个版本号。
 *
 *   node scripts/sync-version.mjs 1.0.0     # 或 v1.0.0，内部会剥掉 v
 *
 * 幂等：同一个版本号跑多次结果一致。CI 里从 git tag 取版本调它，
 * 所以 tag 是版本号的唯一真相源，人不用手动同步这五处。
 */

import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')

/** 去掉可能的 v 前缀，并做基本校验——省得把 "banana" 写进 Cargo.toml。 */
function normalize(input) {
  const v = String(input ?? '').trim().replace(/^v/, '')
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(v)) {
    console.error(`版本号不合法：${JSON.stringify(input)}（应形如 1.0.0 或 v1.0.0）`)
    process.exit(1)
  }
  return v
}

function patch(file, apply, describe) {
  const path = join(ROOT, file)
  const before = readFileSync(path, 'utf8')
  const after = apply(before)
  if (after === before) {
    console.log(`  · ${file.padEnd(30)} 已是 ${describe}，未改动`)
    return false
  }
  writeFileSync(path, after)
  console.log(`  ✓ ${file.padEnd(30)} → ${describe}`)
  return true
}

const version = normalize(process.argv[2])
console.log(`同步版本号 → ${version}`)

let changed = 0

// 1. package.json
changed += patch(
  'package.json',
  (s) => s.replace(/("version"\s*:\s*")[^"]+(")/, `$1${version}$2`),
  version
) ? 1 : 0

// 2. tauri.conf.json —— 留着 "productName" 在前，靠 version 这个键名定位，
//    免得误伤别的同名字段
changed += patch(
  'src-tauri/tauri.conf.json',
  (s) => s.replace(/("version"\s*:\s*")[^"]+(")/, `$1${version}$2`),
  version
) ? 1 : 0

// 3. Cargo.toml —— 只改 [package] 段那行。文件里可能还有别的 version 字段
//    （[profile.*] 等），必须锚在文件开头的 package 段内
changed += patch(
  'src-tauri/Cargo.toml',
  (s) =>
    s.replace(
      /^(\[\s*package\s*\][\s\S]*?^version\s*=\s*")[^"]+(")/m,
      `$1${version}$2`
    ),
  version
) ? 1 : 0

// 4. Cargo.lock —— **绝不能整文件替换**。依赖树里 byteorder-lite 和 vswhom
//    这两个第三方包恰好也是 0.1.0，全局替换会把它们一起改坏，锁文件直接失效。
//    只能先定位 [[package]] 里 name = "zephyr-translator" 的那块，再改紧随其后的
//    version 行。
changed += patch(
  'src-tauri/Cargo.lock',
  (s) =>
    s.replace(
      /(\[\[package\]\]\nname = "zephyr-translator"\nversion = )"[^"]+"/,
      `$1"${version}"`
    ),
  version
) ? 1 : 0

// 5. SettingsView.vue 的手写常量——这个最容易漏，因为它不参与任何构建校验
changed += patch(
  'src/views/SettingsView.vue',
  (s) => s.replace(/(const APP_VERSION = ')[^']+(')/, `$1${version}$2`),
  version
) ? 1 : 0

console.log(changed === 0 ? '（本来就是这个版本）' : `共改动 ${changed} 个文件`)

// 自检：Cargo.lock 里那两个第三方包的版本必须还在，别被误伤
const lock = readFileSync(join(ROOT, 'src-tauri/Cargo.lock'), 'utf8')
for (const dep of ['byteorder-lite', 'vswhom']) {
  const m = lock.match(
    new RegExp(`\\[\\[package\\]\\]\\nname = "${dep}"\\nversion = "([^"]+)"`)
  )
  if (!m) {
    console.error(`自检失败：Cargo.lock 里找不到 ${dep}，锁文件可能被改坏了`)
    process.exit(1)
  }
}
console.log('自检通过：第三方包的版本未被误伤')
