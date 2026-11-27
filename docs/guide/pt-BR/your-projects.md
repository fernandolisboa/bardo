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

Antes de a pasta de um projeto ir para a Lixeira, o Bardo grava nela os registros do projeto, em **bardo-project.db**. Restaurada da Lixeira, a pasta abre de novo com **Abrir projeto de uma pasta…** ([Abrir um projeto de uma pasta](#open-folder)). Se os registros não puderem ser gravados, nada é excluído. Uma pasta excluída de vez não leva registros.

Nada é excluído nas redes, e [Custos](costs.md#deleted) guarda o gasto do projeto com o nome dele até você esquecê-lo.

Arquivar e excluir são recusados enquanto algo está pendente, e o diálogo lista o que resolver antes:

- uma publicação esperando o horário no Bardo ([agendamento](uploading.md)),
- uma publicação esperando na rede para ficar pública mais tarde,
- uma tarefa do projeto, de um dos canais ou vídeos dele, na fila ou em andamento ([Tarefas](jobs.md)).

<a id="export"></a>
## Exportar um projeto

**Exportar projeto…** no seletor grava o projeto aberto num único arquivo **.bardo-project**, onde você escolher, como uma tarefa com progresso e **Cancelar** ([Tarefas](jobs.md)). O pacote leva:

- os canais, ideias, vídeos, mundo, narradores, produtos e modelos do projeto, com as publicações como histórico;
- cópias dos narradores, produtos, personagens e modelos globais que o projeto usa, para ele abrir completo em outro lugar;
- a mídia dos vídeos e do mundo dele.

Ele nunca leva chaves de API, credenciais de app, logins nas redes ou tokens, custos, tarefas não terminadas, nem os números que o YouTube deixa os apps guardarem só por um tempo ([Dados e chaves](data-and-keys.md#credentials)). Exporte para levar um projeto a outro computador ou perfil, ou para guardar a cópia de um projeto à parte de um [backup](backup.md) completo.

<a id="import"></a>
## Importar um pacote de projeto

**Importar pacote de projeto…** pede um arquivo **.bardo-project**, mostra o que ele traz, quando e por qual versão do Bardo foi gravado, e pede o nome, a cor e a pasta do projeto novo. O nome é o do pacote, numerado quando você já tem um projeto com esse nome. **Importar** traz o projeto como uma tarefa; ele aparece no seletor quando a tarefa termina.

- Ele sempre entra como um **projeto novo**, com pasta própria e ids novos: importar um pacote no perfil de onde ele saiu cria um segundo projeto ao lado do primeiro.
- Os canais vêm sem as conexões com as redes: conecte de novo em [Contas de rede](network-accounts.md). Um canal com um nome que você já usa ganha o nome do projeto depois dele.
- As publicações vêm como histórico. Uma publicação que este perfil já tem (a mesma rede e o mesmo post) fica de fora, para nada contar duas vezes em [Desempenho](performance-metrics.md).
- Os narradores, produtos e personagens globais que ele trouxe viram do projeto. Os modelos globais também, só quando marcados no diálogo; os outros continuam os globais deste perfil, e os textos já escritos guardam o que os gerou ([Modelos](templates.md)).
- Um pacote gravado por um Bardo mais novo é recusado: atualize o Bardo antes ([Atualizações](about.md#updates)).

<a id="open-folder"></a>
## Abrir um projeto de uma pasta

**Abrir projeto de uma pasta…** pede a pasta de um projeto (a que tem **bardo-project.json**) e abre o projeto dela:

- um projeto que o Bardo conhece, cuja pasta você moveu, passa a usar o lugar novo, como **Localizar…** faz ([Pasta não encontrada](storage.md#not-found));
- a pasta de um projeto excluído, restaurada da Lixeira, volta como um projeto novo a partir dos registros que guarda, com a mídia onde está. Um cuja mídia foi compactada volta arquivado: **Desarquivar** descompacta a mídia.

Uma pasta de outra cópia do Bardo ou de outro perfil é recusada: exporte o projeto lá e importe o pacote aqui.

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
