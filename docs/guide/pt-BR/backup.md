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

<a id="settings-file"></a>
## Arquivo de configurações

Um arquivo de configurações leva suas chaves, preferências, personas e canais para outra cópia do Bardo: de uma compilada do código para a instalada, ou para um computador novo, sem os projetos e posts que um backup leva. Uma senha o protege.

**Exportar configurações…** lista o que esta cópia tem, tudo marcado menos os logins:

- **Chaves de API** e **Credenciais de app**, cada uma salva pelo nome.
- **Preferências e custos**: o idioma, o tema e o layout da interface, se o Bardo oferece tours, faz backup toda semana e procura atualizações, como os números dos posts sincronizam, a pontuação mínima das sugestões de corte, seus orçamentos e os preços que você mudou.
- **Personas** e **Canais**, cada canal com suas contas nas redes.
- **Logins**: o login de uma conta, que só vai junto com o canal dele. Em geral, deixe desmarcado e entre de novo na outra cópia. Algumas redes, como o X, mantêm só uma cópia conectada: a primeira que renovar o login desconecta a outra.

Digite duas vezes uma senha de pelo menos 10 caracteres e selecione **Salvar arquivo…**. O Bardo não recupera uma senha esquecida: sem ela, o arquivo não abre. O arquivo é criptografado com essa senha, então guarde o arquivo e a senha separados; quem tiver os dois consegue ler suas chaves.

**Importar configurações…** pede o arquivo, mostra qual Bardo o fez e quando, e pede a senha. Depois lista o que o arquivo tem, tudo marcado menos os logins, cada item marcado **Novo aqui** ou **Substitui o daqui**. O que substitui fica com os dados do arquivo e mantém o lugar: uma persona ou um canal de mesmo nome continua com seus projetos, e uma chave ou credencial de app da mesma rede é sobrescrita. Nada que você deixar desmarcado, e nada que o arquivo não tenha, muda. **Importar** traz os itens marcados e lista os que não puderam ser salvos.

Um arquivo feito por um Bardo mais novo abre se o formato dele for um que este Bardo conhece; valores ou partes que este Bardo não conhece aparecem numa lista e ficam de fora. Atualize o Bardo para importá-los. Um arquivo de formato mais novo pede que você atualize o Bardo antes ([Atualizar](install.md#update)).

Na primeira vez que uma cópia nova do Bardo abre, uma barra sobre as telas oferece importar um arquivo de configurações; **Agora não** a fecha, e a aba oferece o mesmo a qualquer momento. O instalador também aceita um arquivo de configurações ([Instalação](install.md#setup)).

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
3. Depois de reiniciar, siga a lista: salve de novo suas chaves e credenciais de app e reconecte cada conta. Um [arquivo de configurações](#settings-file) feito no computador antigo traz as chaves e credenciais de app de uma vez.
