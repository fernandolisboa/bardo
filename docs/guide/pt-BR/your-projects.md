---
id: your-projects
title: Seus projetos
group: getting-started
---

# Seus projetos

Um **projeto** reúne os canais de uma marca, com a pesquisa, as ideias e os vídeos deles. Você trabalha em um projeto por vez: o **projeto aberto**. Visão geral, Ideias, Vídeos, Canais e contas e Desempenho mostram só os canais dele e o trabalho deles; narradores, produtos e templates são compartilhados por todos os projetos por enquanto ([Biblioteca global](global-library.md)). [Mostre o seletor](bardo:tour/welcome).

Ao atualizar para esta versão, o Bardo criou um projeto para cada canal, com o nome dele. Canais ligados por uma versão localizada foram juntos para um projeto, com o nome do mais antigo.

<a id="switcher"></a>
## O seletor de projetos

A barra de título mostra o projeto aberto ao lado do logo do Bardo: a cor, o nome e quantos canais ele tem. Clique nele para abrir o seletor:

- **Buscar** filtra a lista enquanto você digita; Enter abre o primeiro projeto encontrado.
- **Favoritos** são os projetos que você marcou com estrela, na ordem que você escolher. A seta sobe um favorito.
- **Recentes** lista os oito últimos projetos que você abriu, do mais recente, sem os favoritos.
- **Todos** lista os demais pelo nome.

Clique em um projeto para abri-lo. O Bardo lembra o projeto aberto e abre nele da próxima vez. **Mostrar pasta** abre a pasta do projeto aberto no Explorer. Um projeto cuja pasta não é encontrada aparece com **Pasta não encontrada**, e **Localizar…** ([Pasta não encontrada](storage.md#not-found)). A estrela ao lado de um projeto o coloca nos favoritos ou tira de lá. O seletor não aparece com o editor aberto: feche o editor para trocar.

<a id="no-project"></a>
## Sem projeto

**Sem projeto**, depois dos projetos no seletor, abre os vídeos avulsos: vídeos sem projeto e sem canal, para um teste rápido ([Vídeos avulsos](projects.md#loose-videos)). Ele mostra quantos são. Enquanto está aberto, a barra de título diz **Sem projeto**, Vídeos lista os vídeos avulsos e as outras telas do projeto pedem para abrir um projeto. O Bardo lembra dele como de qualquer projeto, e abre nele da próxima vez.

<a id="new"></a>
## Um projeto novo

**Novo projeto…**, no fim do seletor, pede um nome, uma cor e uma pasta: a pasta base por padrão, ou outro lugar em **Escolher…**. O formulário mostra a pasta que vai criar ([A pasta de um projeto](storage.md#project-folder)). Os nomes não se repetem entre os seus projetos, com maiúsculas ou não. O projeto novo abre sem canal, e todas as telas pedem um: crie o primeiro canal dele em [Canais](channels.md).

<a id="archive"></a>
## Arquivar um projeto

**Arquivar projeto…** no seletor guarda o projeto aberto: ele sai do seletor, de Recentes e de Favoritos, e fica só para leitura, então nenhuma tarefa dele roda e nada dele é publicado. As publicações dele continuam sincronizando as métricas, e as contas das redes continuam conectadas. Outro projeto abre no lugar.

**Compactar mídia** junta as pastas dos vídeos em **Videos.zip** na pasta do projeto, numa tarefa ([Tarefas](jobs.md)); o Bardo relê o arquivo para conferi-lo antes de remover as pastas. A pasta **World** nunca é compactada.

**Projetos arquivados**, no fim do seletor, lista os arquivados, marcados com **Mídia compactada** quando for o caso. **Desarquivar** traz um de volta, com a estrela se ele tinha; um compactado é descompactado antes, e volta quando essa tarefa termina.

<a id="delete"></a>
## Excluir um projeto

**Excluir projeto…** no seletor (ou a lixeira ao lado de um projeto arquivado) pede que você digite o nome do projeto. Ele remove o projeto com os canais, ideias e vídeos, desconecta as contas das redes como **Desconectar** faz ([Contas das redes](network-accounts.md)) e manda a pasta para a Lixeira. O diálogo avisa antes quando a pasta será excluída de vez: num drive removível ou de rede, quando a Lixeira do drive está desativada, ou quando a pasta é maior do que essa Lixeira comporta. **Excluir de vez** pula a Lixeira de propósito. Uma pasta que o Bardo não alcança fica como está, e só os registros saem.

Nada é excluído nas redes, e [Custos](costs.md#deleted) guarda o gasto do projeto com o nome dele até você esquecê-lo.

Arquivar e excluir são recusados enquanto algo está pendente, e o diálogo lista o que resolver antes:

- uma publicação esperando o horário no Bardo ([agendamento](uploading.md)),
- uma publicação esperando na rede para ficar pública mais tarde,
- uma tarefa do projeto, de um dos canais ou vídeos dele, na fila ou em andamento ([Tarefas](jobs.md)).

<a id="move"></a>
## Mover um canal

Um canal muda de projeto em [Canais](channels.md#project), com **Mover para outro projeto…**. Ele leva junto as ideias, os vídeos, as contas nas redes e a mídia: a mídia vai para a pasta do novo projeto numa tarefa ([Mudança de mídia](storage.md#moving-media)). O Bardo recusa enquanto houver algo pendente, e diz o quê:

- uma publicação esperando o horário no Bardo ([agendamento](uploading.md)),
- uma publicação esperando na rede para ficar pública mais tarde,
- uma tarefa do canal ou de um vídeo dele, na fila ou em andamento ([Tarefas](jobs.md)),
- um vídeo do canal que é versão localizada de um vídeo de outro canal do projeto, ou tem uma versão lá. Versões ficam dentro de um projeto, então uma versão só é oferecida para os outros canais do projeto.

<a id="jobs-costs"></a>
## Tarefas e custos

[Tarefas](jobs.md#filter) e [Custos](costs.md#projects) continuam sendo do perfil inteiro: tarefas rodam e dinheiro é gasto seja qual for o projeto aberto. As duas telas têm um filtro de projeto que começa no projeto aberto, e cada tarefa e cada custo guardam os nomes do projeto, do canal e do vídeo para que foram, como eram na época.
