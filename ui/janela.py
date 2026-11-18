"""Janela do Orion: chat, orb animado, arrastar arquivo.
tkinter (vem no Python). Arrastar usa tkinterdnd2 se instalado; senao, botao +.
A conversa roda numa thread e tudo que toca na UI passa por uma fila,
porque tkinter nao e thread-safe.
"""
import math
import queue
import threading
from pathlib import Path

import tkinter as tk
from tkinter import filedialog, messagebox

from core import agente, config, memory, scheduler
from senses import voz, watcher

try:
    from tkinterdnd2 import DND_FILES, TkinterDnD
    _DND = True
except Exception:
    _DND = False

BG, PAINEL, TXT, MUTE = "#0d1117", "#161b22", "#e6edf3", "#7d8590"
VERDE, AMAR, VERM, AZUL = "#3fb950", "#d29922", "#f85149", "#58a6ff"


def _mix(cor, f):
    a = [int(cor[i:i + 2], 16) for i in (1, 3, 5)]
    b = [int(BG[i:i + 2], 16) for i in (1, 3, 5)]
    return "#" + "".join(f"{int(x * f + y * (1 - f)):02x}" for x, y in zip(a, b))


class Orb:
    """Aneis pulsando + particulas orbitando.
    Azul = parado, amarelo = pensando, verde = falando."""

    def __init__(self, cv, cx, cy):
        self.cv, self.cx, self.cy, self.t, self.cor = cv, cx, cy, 0.0, AZUL
        # do maior pro menor: o que e criado depois fica por cima
        self.aneis = [cv.create_oval(0, 0, 0, 0, outline="", fill=BG) for _ in range(4)]
        self.pontos = [cv.create_oval(0, 0, 0, 0, outline="", fill=BG) for _ in range(6)]
        self._anima()

    def _anima(self):
        agitado = self.cor != AZUL
        self.t += 0.20 if agitado else 0.07
        pulso = (math.sin(self.t) + 1) / 2
        for i, anel in enumerate(self.aneis):
            k = 3 - i  # 3 = maior
            r = 18 + k * 10 + pulso * (3 + k * 2)
            self.cv.coords(anel, self.cx - r, self.cy - r, self.cx + r, self.cy + r)
            self.cv.itemconfig(anel, fill=_mix(self.cor, 0.9 - k * 0.22))
        for i, pt in enumerate(self.pontos):
            ang = self.t * (1.6 if agitado else 0.6) + i * math.pi / 3
            raio = 50 + math.sin(self.t * 2 + i) * 4
            x, y = self.cx + math.cos(ang) * raio, self.cy + math.sin(ang) * raio * 0.45
            self.cv.coords(pt, x - 2.5, y - 2.5, x + 2.5, y + 2.5)
            self.cv.itemconfig(pt, fill=_mix(self.cor, 0.85))
        self.cv.after(40, self._anima)


class Janela:
    def __init__(self):
        self.root = TkinterDnD.Tk() if _DND else tk.Tk()
        self.root.title("Orion")
        self.root.configure(bg=BG)
        self.root.geometry("460x640")
        self.root.minsize(360, 480)
        self.fila = queue.Queue()
        self.anexos = []
        self.pensando = False
        self._monta()
        memory.marcar_inicio()
        scheduler.vigiar(lambda t: self.fila.put(("alerta", f"Lembrete: {t}")))
        watcher.vigiar(lambda t: self.fila.put(("alerta", t)))
        self.root.protocol("WM_DELETE_WINDOW", self._fechar)
        self.root.after(80, self._drenar)
        dica = "arraste um arquivo aqui" if _DND else "+ anexa arquivo"
        self._dizer(f"online · voz: {voz.motor_atual()} · {dica}", "info")
        if not config.API_KEY:
            self._dizer("Sem chave do Gemini: fecha e roda  orion chave SUA_CHAVE", "aviso")

    def _monta(self):
        cv = tk.Canvas(self.root, width=460, height=130, bg=BG, highlightthickness=0)
        cv.pack(fill="x")
        self.orb = Orb(cv, 230, 66)

        self.log = tk.Text(self.root, bg=PAINEL, fg=TXT, bd=0, wrap="word",
                           font=("Segoe UI", 11), padx=12, pady=10, state="disabled",
                           highlightthickness=0)
        self.log.pack(fill="both", expand=True, padx=10, pady=(0, 8))
        for tag, cor in (("orion", VERDE), ("chefe", AZUL), ("aviso", AMAR), ("erro", VERM)):
            self.log.tag_config(tag, foreground=cor)
        self.log.tag_config("info", foreground=MUTE, font=("Consolas", 9))

        baixo = tk.Frame(self.root, bg=BG)
        baixo.pack(fill="x", padx=10, pady=(0, 10))
        tk.Button(baixo, text="+", width=3, bg=PAINEL, fg=TXT, bd=0, relief="flat",
                  highlightthickness=0, cursor="hand2", font=("Segoe UI", 13),
                  activebackground="#30363d", activeforeground=TXT,
                  command=self._anexar).pack(side="left", padx=(0, 6), ipady=2)
        self.entrada = tk.Entry(baixo, bg=PAINEL, fg=TXT, bd=0, relief="flat",
                                font=("Segoe UI", 11), insertbackground=TXT,
                                highlightthickness=1, highlightbackground="#30363d",
                                highlightcolor=AZUL)
        self.entrada.pack(side="left", fill="x", expand=True, ipady=8)
        self.entrada.bind("<Return>", lambda e: self._enviar())
        self.entrada.focus()
        tk.Button(baixo, text="➤", width=3, bg=VERDE, fg=BG, bd=0, relief="flat",
                  highlightthickness=0, cursor="hand2", font=("Segoe UI", 12, "bold"),
                  activebackground="#2ea043", command=self._enviar).pack(side="left", padx=(6, 0), ipady=2)

        if _DND:
            for w in (self.log, self.entrada):
                w.drop_target_register(DND_FILES)
                w.dnd_bind("<<Drop>>", self._soltou)

    # ---------- saida: so na thread da UI ----------
    def _dizer(self, texto, tipo="info"):
        self.log.config(state="normal")
        rotulo = {"orion": "Orion", "chefe": config.CHEFE}.get(tipo)
        if rotulo:
            self.log.insert("end", f"{rotulo}: ", tipo)
        self.log.insert("end", texto[:2000] + "\n\n", tipo)
        self.log.config(state="disabled")
        self.log.see("end")

    def _drenar(self):
        try:
            while True:
                item = self.fila.get_nowait()
                if item[0] == "confirmar":
                    _, tool, args, motivo, ev, resp = item
                    resp["v"] = messagebox.askyesno(
                        "Orion", f"Rodar {tool}?\n\n{args}\n\n{motivo}".strip(),
                        icon="warning", parent=self.root)
                    ev.set()
                elif item[0] == "alerta":  # watcher/lembrete: texto + voz
                    self._dizer(item[1], "orion")
                    voz.falar(item[1])
                else:
                    self._dizer(item[1], item[0])
        except queue.Empty:
            pass
        self.orb.cor = AMAR if self.pensando else VERDE if voz.falando() else AZUL
        self.root.after(80, self._drenar)

    # ---------- anexos ----------
    def _anexar(self):
        for p in filedialog.askopenfilenames(parent=self.root):
            self._por_anexo(p)

    def _soltou(self, ev):
        for p in self.root.tk.splitlist(ev.data):
            self._por_anexo(p)

    def _por_anexo(self, p):
        self.anexos.append(p)
        self._dizer(f"anexado: {Path(p).name}", "info")

    # ---------- envio ----------
    def _enviar(self):
        texto = self.entrada.get().strip()
        if self.pensando or (not texto and not self.anexos):
            return
        texto = texto or "da uma olhada nisso"
        anexos, self.anexos = self.anexos, []
        self.entrada.delete(0, "end")
        voz.parar()
        self._dizer(texto, "chefe")
        self.pensando = True
        threading.Thread(target=self._rodar, args=(texto, anexos), daemon=True).start()

    def _rodar(self, texto, anexos):
        def dizer(t, tipo="info"):
            self.fila.put((tipo, t))

        def confirmar(tool, args, motivo):
            ev, resp = threading.Event(), {}
            self.fila.put(("confirmar", tool, args, motivo, ev, resp))
            ev.wait()
            return resp.get("v", False)

        try:
            agente.responder(texto, dizer, confirmar, voz.falar, anexos)
        except Exception as e:
            self.fila.put(("erro", f"deu ruim: {type(e).__name__}: {e}"))
        finally:
            self.pensando = False

    def _fechar(self):
        voz.parar()
        self.root.destroy()

    def loop(self):
        self.root.mainloop()
        print("guardando o que importa da conversa...")
        if memory.consolidar():
            print(f"memoria atualizada: {memory.RESUMO}")


def abrir():
    Janela().loop()
