# Benchmarks: perseu (GPUI) contra o Perseu (Chromium)

Local only: os resultados e perfis ficam fora de qualquer repositório (veja `.gitignore`).

```
node benchmark/smoke.mjs                 # valida a automação antes de gastar tempo com a suíte
node benchmark/run.mjs                   # 3 runs, uns 20 minutos
node benchmark/run.mjs --speedometer     # adiciona Speedometer 3.1, ~5 min por run
node benchmark/run.mjs --runs 5
node benchmark/run.mjs --app <perseu.exe>
```

Ao final, o `run.mjs` compara com o resultado mais recente do Perseu Chromium
(`flecha-browser/benchmarks/results/`, ou passe `--baseline <arquivo.json>`) e escreve
`results/<carimbo>.comparison.md` junto com o JSON e a tabela do run. Para refazer só a
comparação: `node benchmark/compare.mjs <results.json>`.

O navegador GPUI (`perseu`) não tem controle remoto próprio, então nada do código dele foi
tocado. A suíte o dirige por duas portas dos fundos que já existem:

- **Webviews**: o runtime WebView2 abre um servidor DevTools quando recebe
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=…` (variável de ambiente, lida na
  criação do ambiente). Por ele vêm o FCP de cada página e os frames do screencast.
- **Shell**: os atalhos do próprio navegador (`ctrl+t`, `ctrl+w`, `ctrl+1`… `ctrl+9`) são digitados
  por `probe.ps1`/`probe.cs` com SendInput, e a URL vai pela área de transferência, colada na
  command bar que o navegador abre sozinho.

O perfil fica em `.profiles/perseu/` (WebView2 recebe `WEBVIEW2_USER_DATA_FOLDER`, também sem
código). Cada run parte de uma cópia dele, sempre a frio. Configure-se uma vez (`prepare`, no
primeiro run) e o resto mede o navegador como ele é no dia a dia.

## Os testes

| Teste | Como é medido |
|---|---|
| Cold launch → window on screen | Do start do processo à primeira janela visível (`probe.cs`, EnumWindows). |
| Cold launch → page painted | Do start do processo ao first-contentful-paint da página. A página entra pela command bar que o navegador abre no startup; o FCP é histórico, então vale a pintura real da página. |
| Idle CPU | Tempo de CPU do processo e de todos os filhos (webviews inclusos) por 15 s, como % de um núcleo: 1 aba 5 s depois do launch, 1 aba um minuto depois ("settled"), 20 abas, e 20 abas um minuto depois. |
| Idle wake-ups | Trocas de contexto por segundo de todas as threads, nas mesmas janelas (o Windows não tem contador de wake-ups; isso é o mais próximo). |
| New tab → first paint | Do `ctrl+t` ao FCP da página que abre. Passa pela command bar (colar + Enter), que é o fluxo real de nova aba deste navegador — então carrega esse passo a mais que o número do Chromium, que abre a aba por API. |
| Tab switch → shown | Do atalho (`ctrl+1` / `ctrl+2`) ao primeiro frame pintado da aba mostrada, por screencast do DevTools: webview escondido não pinta nada, então o primeiro frame que chega depois do atalho é o primeiro frame exibido. (O `visibilitychange` não dispara em webview hospedado como janela filha — quirk do WebView2 — por isso o screencast.) |
| Memory | Private working set ("Memória" do Gerenciador de Tarefas) de todos os processos, com 1, 10 e 20 abas, e de volta a 1. |
| Speedometer 3.1 | browserbench.org, com `--speedometer`. Maior é melhor. Abre pela command bar, roda sozinho. |

## O que não é comparável direto com o Chromium

- **New tab e cold-launch-painted** carregam o fluxo de teclado (command bar + colar + Enter) que o
  Chromium não tem (URL pela linha de comando, aba por API). Leia como "fluxo GPUI" contra "API".
- **Tab switch** usa screencast (timestamp do compositor) porque o `visibilitychange`+rAF do
  Chromium não existe aqui.
- **New window → first paint**: não existe — o navegador é de janela única. Teste pulado.
- **Abas em segundo plano**: aqui cada aba abre em primeiro plano (o navegador não tem background
  tabs), então cada página pinta antes da próxima abrir.
- O Perseu Chromium roda com uBlock; o perseu não tem bloqueador. No Speedometer isso pesa um
  pouco a favor do GPUI.

Feche programas pesados enquanto roda e não use o teclado: os atalhos do benchmark vão para a
janela em foco, que precisa ser a do navegador.
