// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
"use client";
import React, { useState } from "react";
import { useSettings } from "@/lib/hooks/use-settings";
import { useLocale } from "@/lib/i18n";
import { safeTractionEndpoint } from "@/lib/analytics/traction";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export function TractionSettings() {
  const { settings, updateSettings } = useSettings();
  const ru = useLocale() === "ru";
  const saved = settings.tractionAnalytics;
  const [endpoint, setEndpoint] = useState(saved?.endpoint ?? "https://stats.multitool.works/p/screenpipe");
  const [actor, setActor] = useState(saved?.actor ?? "");
  const [token, setToken] = useState(saved?.token ?? "");
  const [message, setMessage] = useState("");
  const save = async (enabled: boolean) => {
    if (enabled && (!safeTractionEndpoint(endpoint) || !/^[a-zA-Z0-9_-]{8,100}$/.test(actor) || token.length < 20)) {
      setMessage(ru ? "Проверьте адрес, ID установки и индивидуальный ключ." : "Check endpoint, installation ID and individual credential."); return;
    }
    try {
      await updateSettings({ tractionAnalytics: { enabled, endpoint, actor, token: enabled ? token : "" } });
      if (!enabled) setToken("");
      setMessage(ru ? "Настройка сохранена. Уже отправленные события удаляются отдельно администратором." : "Saved. Previously sent events require a separate administrator deletion.");
    } catch { setMessage(ru ? "Не удалось сохранить настройку." : "Could not save settings."); }
  };
  return <section className="border border-border bg-card p-4 space-y-3" data-testid="traction-settings">
    <h3 className="text-sm font-medium">{ru ? "Статистика пилота · Traction" : "Pilot analytics · Traction"}</h3>
    <p className="text-xs text-muted-foreground">{ru ? "По умолчанию выключена. Отправляются названия действий в журнале, их исход (успех или вид ошибки), оценки, вид экрана, время события, версия приложения, ОС и анонимный ID установки. Текст карточек, экрана, аудио, заметок, ссылки, пути и цели не отправляются. Индивидуальный ключ выдаёт администратор пилота. Общий ключ сервера здесь использовать нельзя." : "Off by default. Sent: journal action names, their outcome (success or error kind), ratings, which view, event time, app version, OS and an anonymous installation ID. Never sent: card, screen, audio or note text, URLs, file paths or intentions. Use an individual pilot credential, never the shared server secret."}</p>
    <label className="block text-xs">{ru ? "Адрес модуля" : "Module URL"}<Input value={endpoint} onChange={e => setEndpoint(e.target.value)} /></label>
    <label className="block text-xs">{ru ? "Анонимный ID установки" : "Anonymous installation ID"}<Input value={actor} onChange={e => setActor(e.target.value)} /></label>
    <label className="block text-xs">{ru ? "Индивидуальный ключ" : "Individual credential"}<Input type="password" autoComplete="off" value={token} onChange={e => setToken(e.target.value)} /></label>
    <div className="flex gap-2"><Button size="sm" onClick={() => void save(true)}>{ru ? "Включить сбор" : "Enable collection"}</Button><Button size="sm" variant="outline" onClick={() => void save(false)}>{ru ? "Выключить и очистить очередь" : "Disable and clear queue"}</Button></div>
    <p role="status" className="text-xs text-muted-foreground">{message || (saved?.enabled ? (ru ? "Сбор включён; доставка зависит от доступности модуля." : "Collection enabled; delivery requires a reachable module.") : (ru ? "Сбор выключен." : "Collection disabled."))}</p>
  </section>;
}
