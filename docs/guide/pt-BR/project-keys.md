---
id: project-keys
title: As chaves próprias de um projeto
group: reference
place: settings/project
tour: project-keys
---

# As chaves próprias de um projeto

[Configurações › Este projeto](bardo:go/settings/project) dá ao projeto aberto uma chave de API própria para um provedor, e um orçamento mensal para o que essa chave paga. **Chaves e orçamentos…** no [seletor de projetos](your-projects.md#switcher) também abre a aba. [Mostre a aba](bardo:tour/project-keys).

<a id="own-keys"></a>
## Chaves próprias

Cada provedor tem um cartão. Sem chave própria, o cartão diz **Usa a chave do app**: o trabalho do projeto chama o provedor com a chave de [Configurações › Chaves de API](api-keys.md), como antes. Salvar uma chave aqui faz toda chamada deste projeto usá-la: as etapas que você inicia nos vídeos dele, os visuais dos narradores, os produtos e o mundo dele, e os trabalhos que o [agente em segundo plano](background-agent.md) roda para ele. Um trabalho enfileirado antes de você salvar a chave usa a chave nova quando roda.

A chave fica no Gerenciador de Credenciais do Windows, só para este projeto, nunca no banco de dados do Bardo, e o cartão mostra só os últimos caracteres. **Testar chave** confere a chave com o provedor, **Substituir chave** salva outra e **Remover** volta para a chave do app. Quando este computador não consegue ler uma chave salva (depois de uma [restauração](backup.md#restore), por exemplo), o cartão avisa e o trabalho do projeto com aquele provedor para até você salvá-la de novo: o Bardo nunca passa para a chave do app sem avisar.

O trabalho fora de um projeto (um vídeo avulso, um narrador ou produto global) sempre usa as chaves do app.

<a id="budgets"></a>
## Orçamentos

Um provedor com chave própria do projeto pode ter um orçamento mensal próprio. O que essa chave paga conta só nele, nunca no orçamento do app para o provedor, e o orçamento do app conta só o que a chave do app paga. Como nos orçamentos do app ([Custos](costs.md#budgets)), o Bardo pergunta antes de uma etapa que passaria do orçamento do projeto. Remover a chave remove o orçamento junto.

[Custos](costs.md#project-keys) mostra as chaves próprias de cada projeto no mês, com o que pagaram contra os orçamentos, e cada chamada registra qual chave pagou.

<a id="leaving"></a>
## O que nunca sai do computador

As chaves de um projeto, como as do app, nunca entram num [pacote de projeto](your-projects.md#export), num arquivo de item ou num [arquivo de configurações](backup.md#settings-file). Excluir o projeto remove as chaves do Gerenciador de Credenciais do Windows. Um backup guarda só a anotação de que o projeto tem uma chave: depois de restaurar em outro computador, a lista de restauração mostra cada uma por projeto e provedor, para salvar de novo ([Dados e chaves](data-and-keys.md#credentials)).
