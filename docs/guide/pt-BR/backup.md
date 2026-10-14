---
id: backup
title: Backup e restauração
group: reference
place: settings/backup
tour: backup
---

# Backup e restauração

[Configurações › Backup](bardo:go/settings/backup) salva seu trabalho num arquivo, faz sozinho um backup do banco de dados toda semana e traz um backup de volta. [Mostre a aba](bardo:tour/backup).

<a id="back-up"></a>
## Fazer backup

**Fazer backup…** pergunta onde salvar um arquivo `.zip` (em Documentos, se você não escolher outra pasta) e o cria como uma tarefa, com o andamento na aba e em [Tarefas](jobs.md). O Bardo continua aberto e trabalhando enquanto isso: o banco de dados é copiado num único momento consistente, mesmo que o [agente em segundo plano](background-agent.md) esteja enviando um post.

O backup tem o [banco de dados](data-and-keys.md#database): seus canais, personas, modelos, projetos, roteiros, pesquisas, posts e agendamentos, custos e configurações. Marque **Incluir mídias** para levar também as [pastas dos projetos](data-and-keys.md#projects): narração, imagens, clipes, música, arquivos importados e renders. A aba mostra o tamanho delas. As prévias do editor ficam de fora, porque o editor as gera de novo quando um projeto abre, assim como o cache de amostras de voz, os logs e as [exportações](data-and-keys.md#exports), que já são arquivos seus.

Um backup deixa de fora os dados do YouTube que o Bardo só pode guardar por 30 dias: pesquisa de nichos, ranqueamentos de temas e os números dos posts do YouTube cujo canal não está conectado. Os números do próprio canal conectado ficam ([Política de privacidade](privacy.md#kept)).

Um backup nunca tem chave, credencial de app ou login: eles ficam no [Gerenciador de Credenciais do Windows](data-and-keys.md#credentials), não no banco nem nas pastas. Guarde o arquivo onde quiser, por exemplo num disco externo ou numa pasta na nuvem.

<a id="automatic"></a>
## Backups automáticos

Com **Fazer backup do banco de dados toda semana** ligado, como vem de início, a primeira vez que o Bardo abre na semana ele faz um backup do banco de dados, sem mídias, em `%LOCALAPPDATA%\Bardo\backups`, e guarda os quatro últimos. Eles aparecem na aba, do mais novo ao mais antigo, cada um com **Restaurar**; **Abrir pasta** os mostra no Explorador de Arquivos.

Eles ficam no mesmo disco dos seus dados: protegem contra um engano ou uma atualização ruim, não contra um disco com defeito ou um computador perdido. Guarde também um backup com mídias em outro lugar.

<a id="restore"></a>
## Restaurar

**Restaurar de um arquivo…** pede um backup e o lê antes. O Bardo recusa um arquivo que não é backup do Bardo, um backup danificado e um feito por um Bardo mais novo (atualize o Bardo antes, veja [Atualizar](install.md#update)). Fora isso, mostra quando o backup foi feito, com qual versão e o que ele tem, e pede sua confirmação.

Restaurar substitui tudo no Bardo pelo backup: o trabalho feito depois dele se perde. Um backup sem mídias mantém as pastas dos projetos deste computador como estão. Com tarefas em andamento a restauração espera: deixe terminarem ou cancele em [Tarefas](jobs.md).

**Restaurar e reiniciar** extrai e confere o backup ao lado dos seus dados, espera o agente em segundo plano terminar o que está enviando e reinicia o Bardo, que coloca o backup no lugar antes de abrir qualquer coisa. Se os arquivos não puderem ser substituídos, o Bardo põe tudo de volta e abre seus dados como estavam, avisando. No caso raro de não conseguir pôr um arquivo de volta, nada é apagado: a aba mostra a pasta com seus dados anteriores. Só uma janela do Bardo restaura: com duas abertas, restaure pela primeira.

Os posts agendados voltam como estavam no backup. Um post cujo horário chegou enquanto era enviado, ou depois que o backup foi feito, aparece como [perdido](missed-posts.md), então o Bardo confere com a rede antes de enviar qualquer coisa e nunca o envia de novo por conta própria. Um envio que estava esperando ou em andamento quando o backup foi feito, e que o Bardo não agenda por conta própria (YouTube, TikTok ou um post do Instagram enviado na hora), aparece com falha em vez de recomeçar, porque pode ter ido depois do backup: confira na rede e tente de novo se o post não estiver lá. Um post que você enviou à mão depois do backup volta a aparecer como agendado: cancele-o.

<a id="after-restore"></a>
## Depois de restaurar

A aba lista o que este computador ainda precisa: as chaves e credenciais de app que o computador do backup tinha e este não tem, e as contas cujo login não está aqui. Cada linha abre o lugar para resolver: [Chaves de API](api-keys.md), [Credenciais de app das redes](app-credentials.md) ou [Contas](network-accounts.md#connect). No mesmo computador costuma não haver nada a fazer. **Concluir** fecha a lista.

<a id="moving"></a>
## Trocar de computador

1. No computador antigo, faça um backup com **Incluir mídias** marcado e copie o arquivo para o novo.
2. [Instale o Bardo](install.md) no computador novo, abra e restaure o arquivo em **Configurações › Backup**.
3. Depois de reiniciar, siga a lista: salve de novo suas chaves e credenciais de app e reconecte cada conta.
