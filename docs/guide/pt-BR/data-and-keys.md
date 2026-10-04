---
id: data-and-keys
title: Onde ficam seus dados e chaves
group: reference
---

# Onde ficam seus dados e chaves

O Bardo guarda tudo no seu computador, na sua conta do Windows. Não existe servidor do Bardo nem conta do Bardo: o que sai do computador vai direto aos provedores e às redes que você usa, com as suas próprias chaves e logins.

<a id="database"></a>
## O banco de dados

Seus canais, personas, modelos, projetos e os roteiros deles, pesquisas, ideias, custos, tarefas, posts e configurações ficam num único arquivo de banco de dados, `%APPDATA%\Bardo\bardo.db`. Ele não guarda nenhuma chave, chave secreta nem login.

<a id="projects"></a>
## Pastas dos projetos

A mídia de cada projeto de vídeo fica numa pasta própria em `%LOCALAPPDATA%\Bardo\projects`: a narração, as imagens, os clipes, os arquivos importados, as cópias de pré-visualização do editor e os arquivos renderizados. Mídia é grande e pertence a este computador, então fica fora de um perfil móvel do Windows. As amostras de voz são um cache em `%LOCALAPPDATA%\Bardo\voice-samples`: só as mais recentes ficam, e uma mais antiga é feita de novo quando você toca.

<a id="exports"></a>
## Exportações

As exportações vão para `Vídeos\Bardo`, uma pasta por projeto com uma pasta por rede dentro, com o arquivo renderizado e o `metadata.txt` dele, onde é fácil arrastar para o envio da rede. Os pacotes de persona vão para onde você salvar, em Documentos a não ser que você escolha outra pasta.

<a id="credentials"></a>
## Gerenciador de Credenciais do Windows

Suas chaves de API, as credenciais de app das redes e os logins das contas conectadas ficam no Gerenciador de Credenciais do Windows, que só a sua conta do Windows lê, em **Credenciais genéricas**, com nomes que começam com `Bardo/`. Elas ficam neste computador: em outro, salve as chaves e as credenciais de novo e reconecte as contas. Remover uma chave ou uma credencial, ou desconectar uma conta, também apaga do Gerenciador de Credenciais.

<a id="logs"></a>
## Logs

O Bardo registra o que faz em `%LOCALAPPDATA%\Bardo\logs\bardo.log`, com tamanho limitado. Cada linha passa pelo mesmo mascaramento da tela: nenhuma chave, chave secreta ou token é escrito nele.

<a id="backup"></a>
## Backup e troca de computador

Para fazer backup do seu trabalho, copie o `bardo.db` e a pasta `projects` com o Bardo fechado. Chaves, credenciais e logins não estão neles, de propósito: depois de restaurar em outro computador, salve tudo de novo nas [Configurações](settings.md#tabs) e reconecte cada conta em [Contas](network-accounts.md#connect).
