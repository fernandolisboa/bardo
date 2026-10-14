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

A versão do Bardo neste computador. Informe-a quando relatar um problema.

<a id="updates"></a>
## Atualizações

O Bardo procura uma versão mais nova no [GitHub](https://github.com/fernandolisboa/bardo/releases) ao abrir e uma vez por dia enquanto fica aberto. Quando sai uma, uma barra sobre as telas avisa: **Ver novidades** abre esta seção, e **Depois** esconde a barra até o Bardo abrir de novo ou sair uma versão mais nova. Nada é baixado nem instalado até você escolher.

- **Procurar atualizações automaticamente** liga ou desliga as buscas ao abrir e diárias. **Procurar agora** busca na hora, mesmo com ela desligada. A linha abaixo diz quando o Bardo buscou pela última vez e o que encontrou.
- Uma versão mais nova mostra o número, quando foi publicada, as novidades e um link para a **Página da versão**.
- **Baixar** busca o instalador como uma tarefa, com o progresso aqui e em [Tarefas](jobs.md). O arquivo é conferido com o checksum SHA-256 publicado na versão; um arquivo que não confere é apagado e nunca roda. Um download interrompido não deixa nada para trás.
- **Instalar…** abre uma revisão. **Instalar e reiniciar** confere o arquivo mais uma vez, abre o instalador e fecha o Bardo; o instalador atualiza o Bardo sem o assistente, mantendo seus dados, configurações e a escolha sobre o agente em segundo plano, e abre o Bardo de novo ao terminar.

A instalação espera enquanto houver tarefas rodando nesta janela (espere terminarem, ou cancele em Tarefas), durante uma [restauração](backup.md#restore) e enquanto outra janela do Bardo estiver aberta. Se **Publicar com o Bardo fechado** estiver ligado, o [agente em segundo plano](background-agent.md) também fecha durante a instalação e volta em até 15 minutos.

A busca usa a API pública do GitHub sem conta: nenhuma chave, nenhum login, e nada sobre você ou seus projetos é enviado. Uma cópia da Microsoft Store nunca busca, porque a Store a atualiza. Uma cópia compilada a partir do código-fonte avisa quando sai uma versão e leva até ela: atualize o código-fonte para tê-la. Você sempre pode [atualizar manualmente](install.md#update).

<a id="privacy"></a>
## Privacidade e YouTube

O Bardo usa os Serviços de API do YouTube, então pede que você aceite a [política de privacidade](privacy.md) dele antes de qualquer recurso do YouTube funcionar: salvar a chave da YouTube Data API ou as credenciais de app do YouTube, conectar um canal, a pesquisa de nichos, sincronizar posts do YouTube e enviar vídeos. Esta seção mostra se você aceitou, e quando.

- **Ler a política de privacidade** abre a política no Guia; **Política de privacidade online** abre a mesma página no site do guia, o endereço a informar nos formulários do Google.
- **Aceitar a política de privacidade** registra que você aceitou esta versão. Quando a política muda, os recursos do YouTube voltam a esperar até você aceitar a nova, e uma barra sobre as telas avisa.
- Os links abaixo levam aos [Termos de Serviço do YouTube](https://www.youtube.com/t/terms), com os quais você concorda ao usar os recursos do YouTube no Bardo, à [Política de Privacidade do Google](https://www.google.com/policies/privacy) e à [página de acesso](https://security.google.com/settings/security/permissions) da sua conta Google, onde você remove o acesso do Bardo.
- **Apagar dados do YouTube…** pergunta antes e então apaga na hora a pesquisa de nichos, os ranqueamentos de temas e os números de posts que o Bardo leu do YouTube ([Apagar seus dados do YouTube](privacy.md#delete)). Nada muda no YouTube.

O Bardo também apaga sozinho, uma vez por dia, o que as regras do YouTube não deixam mais guardar ([Onde fica guardado, e por quanto tempo](privacy.md#kept)).

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
