---
id: storage
title: Armazenamento
group: reference
place: settings/storage
tour: storage
---

# Armazenamento

[Configurações › Armazenamento](bardo:go/settings/storage) escolhe a **pasta base**, onde cada projeto novo ganha a pasta dele. [Mostre a aba](bardo:tour/storage).

<a id="base-folder"></a>
## A pasta base

A pasta base é `Documentos\Bardo`, a menos que você escolha outra em **Alterar…**. **Usar a padrão** volta para ela. Os projetos que você já tem mantêm as pastas deles: a pasta base só decide onde ficam os novos.

Quando o OneDrive sincroniza sua pasta Documentos, a padrão passa a ser sua pasta de usuário (`C:\Users\<você>\Bardo`), e a aba diz por quê: sincronizar gigabytes de clipes enquanto o Bardo os grava falha e enche seu armazenamento na nuvem. Você ainda pode escolher uma pasta sincronizada; a aba então avisa.

Uma cópia do Bardo compilada do código usa `Bardo Dev` no lugar de `Bardo`, como faz com os outros dados ([Uma cópia compilada do código](data-and-keys.md#source)).

<a id="project-folder"></a>
## A pasta de um projeto

Cada projeto tem uma pasta, escolhida quando você o cria ([Um projeto novo](your-projects.md#new)). O nome dela é o nome do projeto, com os caracteres que o Windows não aceita em nome de pasta trocados e cortado em 40 caracteres; quando já existe uma pasta com esse nome, um código curto é acrescentado. Renomear o projeto nunca renomeia nem move a pasta. Dentro dela:

- `Videos`, com uma pasta por vídeo, com o título que ele tinha ao ser criado e um código curto. Ela guarda a narração, as imagens, os clipes, a música, os arquivos importados, as prévias do editor e os renders.
- `World`, para as imagens do projeto.
- `bardo-project.json`, que diz ao Bardo de quem é a pasta.

O Bardo recusa um lugar tão fundo que os arquivos de dentro passariam do limite de caminho do Windows, e pede um mais curto. **Mostrar pasta**, no [seletor de projetos](your-projects.md#switcher) e num vídeo, abre a pasta no Explorer.

<a id="moving-media"></a>
## Mudança de mídia

Vídeos feitos antes das pastas de projeto guardavam a mídia em `%LOCALAPPDATA%\Bardo\projects`. Depois da atualização, o Bardo leva cada um para a pasta do vídeo sozinho, numa tarefa que você acompanha, cancela e retoma em [Tarefas](jobs.md). Ele faz o mesmo com os vídeos de um canal que você move para outro projeto ([Mover um canal](your-projects.md#move)). No mesmo disco a pasta é renomeada na hora; entre discos cada arquivo é copiado e conferido antes de o original ser apagado.

Enquanto a mídia muda, nenhuma outra tarefa começa e o [agente em segundo plano](background-agent.md) pausa, então nada lê ou grava uma pasta no meio da mudança. As tarefas que esperavam começam quando ela termina.

<a id="not-found"></a>
## Pasta não encontrada

Quando a pasta de um projeto não é encontrada (foi movida ou renomeada, ou o disco dela não está conectado), o seletor e a Visão geral marcam o projeto com **Pasta não encontrada**. Até você localizá-la, nenhuma tarefa dele começa, aqui ou no agente em segundo plano: elas esperam em [Tarefas](jobs.md), marcadas como aguardando a pasta. Nada é tomado como perdido enquanto isso, então nenhum retrato ou imagem é desenhado de novo. Um post agendado desse projeto também espera, e aparece como [perdido](missed-posts.md) se o horário passar.

**Localizar…** pede a pasta. O Bardo só aceita a pasta cujo `bardo-project.json` nomeia este projeto; uma pasta de outro projeto, ou de outra cópia do Bardo, é recusada. Reconectar o disco também basta: o Bardo vê a pasta de novo quando você abre o seletor.
