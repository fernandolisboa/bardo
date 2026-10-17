---
id: data-and-keys
title: Onde ficam seus dados e chaves
group: reference
---

# Onde ficam seus dados e chaves

O Bardo guarda tudo no seu computador, na sua conta do Windows. Não existe servidor do Bardo nem conta do Bardo: o que sai do computador vai direto aos provedores e às redes que você usa, com as suas próprias chaves e logins.

<a id="database"></a>
## O banco de dados

Seus canais, personas, modelos, projetos e os roteiros deles, pesquisas, ideias, custos, tarefas, posts e configurações ficam num único arquivo de banco de dados, `%APPDATA%\Bardo\bardo.db`. Ele não guarda nenhuma chave, chave secreta nem login. O que o Bardo lê do YouTube fica só pelo tempo que as regras do YouTube permitem ([Política de privacidade](privacy.md#kept)).

<a id="projects"></a>
## Pastas dos projetos

A mídia de cada projeto de vídeo fica numa pasta própria em `%LOCALAPPDATA%\Bardo\projects`: a narração, as imagens, os clipes, os arquivos importados, as cópias de pré-visualização do editor e os arquivos renderizados. Mídia é grande e pertence a este computador, então fica fora de um perfil móvel do Windows. As amostras de voz são um cache em `%LOCALAPPDATA%\Bardo\voice-samples`: só as mais recentes ficam, e uma mais antiga é feita de novo quando você toca. Uma atualização baixada espera em `%LOCALAPPDATA%\Bardo\updates` até ser instalada; o Bardo a apaga quando já roda a versão mais nova ([Atualizações](about.md#updates)).

<a id="exports"></a>
## Exportações

As exportações vão para `Vídeos\Bardo`, uma pasta por projeto com uma pasta por rede dentro, com o arquivo renderizado e o `metadata.txt` dele, onde é fácil arrastar para o envio da rede. Os pacotes de persona vão para onde você salvar, em Documentos a não ser que você escolha outra pasta.

<a id="credentials"></a>
## Gerenciador de Credenciais do Windows

Suas chaves de API, as credenciais de app das redes e os logins das contas conectadas ficam no Gerenciador de Credenciais do Windows, que só a sua conta do Windows lê, em **Credenciais genéricas**, com nomes que começam com `Bardo/`. Elas ficam neste computador: em outro, salve as chaves e as credenciais de novo e reconecte as contas. Remover uma chave ou uma credencial, ou desconectar uma conta, também apaga do Gerenciador de Credenciais.

<a id="logs"></a>
## Logs

O Bardo registra o que faz em `%LOCALAPPDATA%\Bardo\logs\bardo.log`, com tamanho limitado. Cada linha passa pelo mesmo mascaramento da tela: nenhuma chave, chave secreta ou token é escrito nele. O agente em segundo plano escreve `agent.log` ao lado dele. [Exportar diagnóstico](about.md#diagnostics) reúne os dois para anexar a uma issue.

<a id="backup"></a>
## Backup e troca de computador

Faça backup em [Configurações › Backup](backup.md): um arquivo `.zip` com o banco de dados e, se quiser, as pastas dos projetos, feito com o Bardo aberto. O Bardo também faz sozinho um backup do banco de dados toda semana em `%LOCALAPPDATA%\Bardo\backups`. Chaves, credenciais e logins não entram no backup, de propósito: depois de restaurar em outro computador, salve tudo de novo e reconecte cada conta, como a aba lista ([Trocar de computador](backup.md#moving)).

Para levar chaves, credenciais de app, preferências, personas e canais para outra cópia do Bardo sem o resto, exporte um [arquivo de configurações](backup.md#settings-file): ele é criptografado com uma senha que você escolhe, e você marca o que vai e o que entra.

<a id="source"></a>
## Uma cópia compilada do código

O Bardo compilado e rodado a partir do código-fonte guarda os próprios dados, separados do Bardo instalado: `%APPDATA%\Bardo Dev` e `%LOCALAPPDATA%\Bardo Dev` no lugar das pastas `Bardo`, e uma tarefa própria do agente em segundo plano. As chaves e logins dele ficam no Gerenciador de Credenciais do Windows, no perfil dele. A barra de título mostra **Dev**. Testar uma mudança a partir do código nunca mexe no banco de dados do Bardo instalado, e um Bardo instalado mais antigo nunca abre um banco que uma compilação mais nova alterou. Leve configurações de um para o outro com um [arquivo de configurações](backup.md#settings-file).
