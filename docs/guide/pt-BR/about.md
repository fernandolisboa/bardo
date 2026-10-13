---
id: about
title: Sobre o Bardo
group: reference
place: settings/about
tour: about
---

# Sobre o Bardo

[Configurações › Sobre](bardo:go/settings/about) diz qual Bardo você tem e com o que ele é feito, e reúne o necessário para relatar um problema. [Mostre a aba](bardo:tour/about).

<a id="version"></a>
## Versão

A versão do Bardo neste computador. Informe-a quando relatar um problema, e compare com a release mais recente para saber se vale [atualizar](install.md#update).

<a id="ffmpeg"></a>
## FFmpeg

O Bardo lê, pré-visualiza e renderiza vídeo com o [FFmpeg](https://ffmpeg.org), que o instalador coloca na pasta `ffmpeg` ao lado do Bardo. O FFmpeg é software livre sob a GNU Lesser General Public License versão 3 (LGPL v3): o Bardo o executa como programas separados e o distribui sem modificações.

- **Código-fonte do FFmpeg** baixa o código-fonte com que este build do FFmpeg foi feito.
- **Licença (LGPL v3)** abre a licença que vem com ele.

<a id="notices"></a>
## Avisos de terceiros

O Bardo é feito com bibliotecas de código aberto. **Abrir os avisos** abre uma página, instalada com o Bardo, com cada biblioteca e a licença dela. Uma cópia do Bardo compilada a partir do código-fonte não tem essa página.

<a id="diagnostics"></a>
## Diagnóstico

Quando algo der errado, **Exportar diagnóstico** salva um arquivo `.zip`, onde você escolher, para anexar a uma issue no [GitHub](https://github.com/fernandolisboa/bardo/issues). Ele traz:

- os logs do Bardo e do agente em segundo plano, cada um com sua cópia anterior;
- `system.txt`: as versões do Bardo, do FFmpeg e do Windows, o adaptador gráfico, o layout, o tema e o idioma, e a versão do banco de dados;
- `jobs.txt`: quantas tarefas há em cada estado, e as últimas falhas com seus motivos;
- `accounts.txt`: se cada conta está conectada ou precisa reconectar.

Cada chave, chave secreta e token que o Bardo conhece vira `[redacted]`, nomes de contas e ids das redes viram `[account]`, e a sua pasta de usuário do Windows vira `%USERPROFILE%`. Nada é enviado a lugar nenhum: o arquivo fica onde você o salvou até você anexá-lo, e você pode abri-lo antes para ver o que ele traz. Funciona com o Bardo aberto e com o [agente em segundo plano](background-agent.md) rodando.

**Abrir pasta de logs** abre `%LOCALAPPDATA%\Bardo\logs`, onde ficam os logs ([Logs](data-and-keys.md#logs)).
