---
id: products
title: Produtos
group: strategy
place: products
tour: products
---

# Produtos

Um produto é algo que você anuncia: um produto digital, como um app ou um curso, um produto físico, um serviço, ou uma ideia que ainda não existe. Você o descreve uma vez e o reaproveita em todo anúncio, em qualquer canal. Os produtos ficam em [Produtos](bardo:go/products). O briefing, as imagens e o logo não custam nada; [preencher a ficha](#research) com o Claude e [propor ou ranquear ângulos de anúncio](#angles) são pagos, e cada um mostra o custo antes. [Mostre a tela para mim](bardo:tour/products), ou a [parte 2 do tour](bardo:tour/products-sheet), sobre a ficha e as afirmações, e a [parte 3](bardo:tour/products-angles), sobre os ângulos de anúncio.

<a id="library"></a>
## Seus produtos

Na entrada **Produtos** de um projeto, a lista mostra os produtos do projeto e os globais que os anúncios dele usam, marcados como **Global**; a aba **Produtos** da [Biblioteca global](global-library.md) mostra os globais. **Novo produto** abre um briefing vazio, para o projeto na entrada dele ou para a biblioteca global na aba dela; escolher um produto abre ele para edição. Os produtos arquivados ficam escondidos até você marcar **Mostrar arquivados**. Um anúncio de um canal do projeto pode usar os produtos do projeto e os globais: para começar um a partir de um produto global que o projeto ainda não usa, abra esse produto na aba da Biblioteca global, ou use **Trazer para o projeto** para ter uma cópia.

Um produto global é usado por referência: salvar um que anúncios já usam muda ele em todos, em qualquer projeto, então o Bardo lista esses vídeos e pergunta antes. Para mudá-lo só neste projeto, use **Trazer para o projeto**: o Bardo faz uma cópia independente que pertence ao projeto, com as imagens, o logo, o conceito e a ficha, com um nome que você pode mudar (se o nome já existe, o Bardo sugere *Nome (Global)*). Os anúncios que já usam o produto global continuam com ele.

<a id="brief"></a>
## O briefing

O briefing é o que você conta ao Bardo sobre o produto, e a única fonte que um anúncio tem sobre ele:

- **Nome**: único entre os seus produtos, incluindo os arquivados, com até 80 caracteres.
- **Tipo**: produto digital, produto físico, serviço ou ideia. Um anúncio de uma ideia a apresenta como ideia, nunca como algo que já dá para comprar.
- **O que é** (obrigatório): o que ele faz e como, em poucas frases.
- **Para quem é**: quem compra ou usa, e o que essas pessoas querem.
- **Oferta**, **Link** e **Chamada para ação**: o preço ou desconto, o endereço para onde o anúncio aponta e o que o espectador deve fazer. Um link sem `https://` ganha o prefixo; só endereços da web são aceitos.

<a id="facts"></a>
## Fatos que você garante

**Fatos que você garante** recebe uma afirmação por linha, com suas palavras: recursos, números, preços, prazos de entrega. Marcadores e traços colados no começo da linha são removidos e linhas em branco são descartadas; o Bardo mostra quantos fatos leu. Um anúncio só pode afirmar esses fatos até você confirmar outros, então deixe de fora o que você não consegue comprovar. São até 30 fatos de até 300 caracteres cada.

<a id="owner"></a>
## De quem é o produto

**De quem é** diz se o produto é seu ou de um cliente. Isso define como os posts dos anúncios são identificados: como marca própria, ou como parceria paga quando o produto é de um cliente. **Um anúncio precisa dizer** guarda o que todo anúncio diz, como uma frase legal obrigatória; **Um anúncio nunca pode dizer** guarda as palavras, promessas ou comparações que ficam de fora.

<a id="pictures"></a>
## Imagens

Um produto salvo pode ter até três imagens: PNG, JPEG ou WebP, de 600 a 6.000 px de lado, com o lado maior no máximo três vezes o menor, e até 10 MB. Antes de escolher o arquivo, marque **Posso usar esta imagem e a marca que ela mostra em anúncios**; o Bardo guarda essa confirmação, com o horário, junto da imagem. Seu arquivo é guardado exatamente como está.

Cada imagem é uma **Foto** (o produto, a embalagem, um lugar) ou uma **Tela** (uma captura de tela de um app ou site). O Bardo só mostra uma tela como ela é e nunca a redesenha. Uma foto salva com transparência (um PNG ou WebP com canal alfa) é tratada como já recortada e marcada como **Recortada**, pronta para ir sobre outros fundos. O Bardo segue o que o arquivo diz, então só salve uma imagem assim quando o fundo dela for mesmo transparente. **Remover**, embaixo de uma imagem, pergunta antes de tirar a imagem e a confirmação dela. Um produto também pode não ter imagens: os anúncios dele usam então cenas desenhadas e um cartão final só com texto.

<a id="concept"></a>
## Uma imagem de conceito para uma ideia

Um produto do tipo **Ideia** ainda não existe, então não tem foto. Enquanto ele não tem imagem própria, **Imagem de conceito**, embaixo das imagens, pode desenhar uma a partir da descrição: **Desenhar um conceito** mostra a estimativa, depois o Claude escreve um prompt de imagem com o modelo [Prompt de conceito](templates.md) e o gerador de imagens desenha. Diga o modelo o que disser, o prompt sempre pede um produto original, sem marca, logo ou identidade visual que já existam, e sem nenhum texto. As duas chamadas são pagas, conferidas antes com o seu orçamento e registradas em [Custos](costs.md) sem canal; é preciso uma chave do Claude e uma do Gemini. Se parar depois de o Claude escrever o prompt, **Tentar de novo** em [Tarefas](jobs.md) paga só a imagem; **Desenhar de novo** começa do zero e paga as duas. Um desenho que não seria mais usado (o produto ganhou uma imagem ou mudou de tipo no meio) para antes da próxima chamada paga, e o produto não pode ser excluído enquanto um é desenhado.

Um desenho novo espera revisão: **Manter** faz dele a imagem de conceito do produto, no lugar da anterior, e **Descartar** joga fora. **Desenhar de novo** desenha outro. A imagem mantida fica no lugar de uma foto: nas cenas de produto e como referência de uma cena com o [produto em uso](scenes.md#product-check), que você confere com ela do mesmo jeito. Ela aparece marcada como **Imagem de conceito** onde estiver. Quando o produto tem imagem própria, ou deixa de ser uma ideia, essa imagem é usada e a de conceito não.

<a id="logo"></a>
## O logo

O logo opcional é um PNG, com pelo menos 64 px de cada lado e 200 px no lado maior, usado só no cartão final do anúncio. Fundo transparente fica melhor. Ele pede a mesma confirmação das imagens. **Trocar logo…** substitui e **Remover logo** tira depois que você confirma.

As imagens e o logo ficam no banco de dados do Bardo, então todo backup, inclusive o só do banco de dados, leva eles junto ([Backup e restauração](backup.md)). Um arquivo de configurações leva os briefings, mas não as imagens; um produto que ele substitui continua com as imagens que tem ([Arquivo de configurações](backup.md#settings-file)).

<a id="sheet"></a>
## A ficha

**Briefing**, **Ficha** e **Ângulos**, no topo de um produto salvo, alternam entre o que você escreveu, o que o Bardo sabe além disso e os [anúncios que podem sair dali](#angles). A ficha guarda:

- **Afirmações**: o que um anúncio pode dizer sobre o produto, cada uma com a origem.
- **Sobre o produto**: um resumo, a categoria e a aparência.
- **Para quem é**: até 8 grupos de compradores, com o que cada um quer e o que o faz hesitar.
- **Objeções e respostas** e **Alternativas**: até 12 de cada.
- **Páginas lidas**: as páginas que a última pesquisa leu, cada uma com um link.

Tudo, menos a lista de páginas, é seu para editar; **Salvar a ficha** guarda as alterações. **Categoria restrita** marca um produto de uma categoria que as redes restringem: saúde e medicamentos, produtos financeiros, álcool, jogos de azar, emagrecimento, adulto ou político. O Bardo então lembra você de conferir as regras de cada rede antes de publicar, e não bloqueia nada.

Os backups levam a ficha com o resto do banco de dados. Um arquivo de configurações não leva; um produto que ele substitui mantém a ficha que tem.

<a id="research"></a>
## Preencher a ficha

**Preencher a ficha** oferece três caminhos, e você pode usar mais de um:

1. **Pesquisar na web**: o Claude lê primeiro a página do produto, a partir do **Link** do briefing, e depois busca o que é público sobre o produto e o mercado dele. Uma pesquisa faz no máximo 8 buscas e lê no máximo 5 páginas da web, e é instruída a não abrir PDFs nem outros arquivos. Uma segunda chamada transforma as anotações na ficha. Sem link, a pesquisa busca pelo nome do produto.
2. **Escrever a partir do meu briefing**: uma chamada, sem web. O Claude organiza o que o briefing e as imagens já dizem.
3. **Preencher à mão**: sem chamada e sem custo. A ficha começa com os fatos do briefing.

Os dois primeiros precisam da sua chave do Claude ([Suas chaves de API](api-keys.md)) e são pagos. Antes de uma pesquisa começar, o Bardo mostra o máximo que ela pode custar, com todas as buscas e páginas da web no limite; depois que pesquisas já rodaram, mostra também quanto uma costuma custar. Uma pesquisa é cobrada pelos tokens e pelas buscas, e os dois aparecem em [Custos](costs.md#rates). Uma execução que levaria o Claude além do orçamento pergunta antes.

Enquanto uma execução roda, a ficha não pode ser editada e o produto não pode ser excluído, e por um instante depois que você interrompe uma, enquanto o pedido que ela enviou é abandonado; um briefing que você salva nesse meio-tempo é o que a ficha usa. A execução também aparece em [Tarefas](jobs.md), onde você pode pará-la; tentar de novo uma execução que falhou depois da pesquisa mantém a pesquisa e só escreve a ficha outra vez. Uma pesquisa que falhou depois de o Claude cobrar uma parte dela entra em Custos por essa parte, e o Bardo não a roda de novo sozinho. **Tentar de novo** em Tarefas confere o que a execução ainda tem a pagar como numa execução nova: quando uma nova pesquisa pelo máximo que pode custar, ou só a ficha, alcançaria o orçamento do Claude, o Bardo pergunta antes.

Uma execução acrescenta à ficha: nunca muda o que você escreveu ou revisou, então **Pesquisar de novo** é seguro depois de mudar o briefing. Uma categoria restrita que você escolheu fica, inclusive **Nenhuma**. Um grupo, uma objeção, uma alternativa ou uma página que você removeu não fica na memória, então uma execução posterior pode acrescentar de novo. Uma ficha tem no máximo 80 afirmações, sem contar as rejeitadas; uma execução que encontra a ficha cheia diz quantas afirmações deixou de fora.

<a id="claims"></a>
## Afirmações

Um anúncio só afirma o que está **confirmado**. Uma afirmação é uma frase de até 300 caracteres e um tipo: benefício, característica, prova, oferta ou comparação.

- Os fatos do briefing e as afirmações que você adiciona em **Adicionar afirmação** começam confirmados. Um fato que você acrescenta ao briefing depois entra na ficha quando você salva o briefing; quando uma afirmação de um modelo já diz o mesmo, ela passa a ser sua, confirmada.
- O que um modelo escreveu espera em **Aguardando você**, com a origem: a leitura que o Claude fez do seu briefing, a página do produto (o endereço do seu link, ou um abaixo dele) ou outra página da web. **Abrir a página** mostra de onde veio; uma afirmação lida do briefing não tem página por trás, então confira com o que você sabe. **Confirme** só quando for verdade e você puder comprovar; **Rejeite** o resto. **Editar** muda o texto ou o tipo antes de você decidir.
- Uma afirmação rejeitada continua na lista, para a próxima pesquisa não trazê-la de volta como nova. **Trazer de volta** devolve às que aguardam uma afirmação escrita por um modelo, e às confirmadas uma que é sua.

**Sensível** marca uma afirmação que pede cuidado: um número, um superlativo, uma promessa sobre saúde, dinheiro ou resultados, uma garantia, um prazo ou um concorrente citado pelo nome. Os modelos marcam enquanto escrevem, e você pode marcar ou desmarcar; um número que você escreve em uma afirmação que tinha desmarcado marca de novo. Uma afirmação sensível pode ser confirmada e usada; cada uma que um vídeo usa é listada de novo antes de o vídeo ser publicado.

<a id="angles"></a>
## Ângulos de anúncio

**Ângulos**, a terceira aba de um produto salvo, é onde um anúncio dele começa. Um ângulo de anúncio é um jeito de um anúncio falar com o público de um canal: o que ele diz, o formato que segue, a parte do público com quem fala, um rascunho das primeiras palavras e as afirmações confirmadas que ele usa.

Escolha o canal em **Para o canal**: um ângulo é escrito para o público de um canal, no idioma dele, e cada canal guarda os próprios ângulos do produto. **Propor ângulos de anúncio** começa então uma execução. O Claude propõe cinco ângulos com o modelo **Ângulos de anúncio** ([Modelos](templates.md#kinds)), só a partir do briefing do produto, da ficha e das afirmações confirmadas, e evita repetir os que já existem. Depois o motor de decisão ranqueia. A estimativa aparece antes de a execução começar e conta os ângulos que já esperam um ranqueamento, que a mesma execução ranqueia; uma execução que levaria um provedor além do orçamento pergunta antes, e **Tentar de novo** em [Tarefas](jobs.md) também pergunta, pelo que uma execução que falhou ainda tem a pagar. As duas chamadas aparecem em [Custos](costs.md), no canal. Você precisa de uma chave do Claude e da chave do motor de decisão ([Chaves de API](api-keys.md)). Enquanto uma execução roda, o produto não pode ser excluído; uma execução que não encontra mais o produto ou o canal para antes de pagar por qualquer coisa.

Um produto recebe ângulos quando a ficha tem pelo menos uma afirmação confirmada, porque um anúncio não diz mais nada; a aba mostra o que falta. Um produto sem imagens pode ter ângulos; um arquivado não. Um ângulo proposto pelo Claude que não usa nenhuma das afirmações confirmadas não é guardado.

Cada ângulo segue um de nove formatos: problema e solução, demonstração, antes e depois, como funciona, a história por trás, jeito antigo e jeito novo, três motivos, mito e fato, ou oferta. Não existe formato de depoimento: um anúncio nunca apresenta um cliente inventado.

<a id="angle-review"></a>
## Ranquear e revisar um ângulo

O motor de decisão responde três perguntas sobre cada ângulo, cada uma com uma nota de 0 a 100 e o quanto ele tinha certeza: a força do **gancho**, o **encaixe com o público** que o ângulo indica e a **clareza**. A **Prioridade** pesa o gancho 40%, o encaixe com o público 35% e a clareza 25%, e a lista mostra o melhor primeiro; a **Confiança** é a da resposta de que o motor tinha menos certeza. **Usa uma afirmação sensível** é uma marca do próprio Bardo, não de um modelo: uma das afirmações do ângulo está marcada como sensível na ficha.

- **Aprovar e começar o anúncio** começa um projeto de anúncio naquele canal, com o título do ângulo; o projeto começa pela etapa [Briefing](brief.md), e **Abrir o projeto** leva você até lá. O ângulo continua na lista, marcado como aprovado, e começa um projeto só. Aprovar exige o mesmo que propor: um produto que não está arquivado, com uma afirmação confirmada na ficha.
- **Editar** muda o título, o ângulo, o formato, com quem ele fala, as primeiras palavras e as afirmações que ele usa (até 8, só confirmadas). Um ângulo editado perde o ranqueamento; **Ranquear ângulos** ranqueia os que estão esperando.
- **Descartar** tira o ângulo da lista. A aba conta quantos foram descartados.

Os ângulos de anúncio aparecem aqui e não em [Temas](themes-ranking.md), e um projeto de anúncio não entra no desempenho passado de um canal: como um anúncio foi não diz nada sobre como vão as ideias do próprio canal.

<a id="archive"></a>
## Duplicar, arquivar e excluir

**Duplicar** salva as alterações que você fez e depois copia o briefing, as imagens, a imagem de conceito mantida, o logo e a ficha com um novo nome, para uma variação do mesmo produto. **Arquivar** esconde da lista e de novos anúncios um produto que você não anuncia mais; **Restaurar** traz ele de volta. **Excluir…** remove o produto com as imagens, a imagem de conceito, o logo e a ficha depois que você confirma, e é recusado enquanto algum vídeo usa o produto (arquive em vez disso), enquanto uma execução preenche a ficha, enquanto os ângulos dele são propostos ou ranqueados, ou enquanto a imagem de conceito dele é desenhada.

<a id="sharing"></a>
## Compartilhar com outros projetos

Abaixo das ações, **Compartilhamento** marca um produto como **Compartilhável** para seus outros projetos, exporta uma cópia para um deles, torna o produto global ou o salva em arquivo com as imagens e o logo; **Importar de outro projeto** e **Abrir arquivo de item…**, no topo, trazem um para cá. Uma cópia vinculada segue toda mudança do original, inclusive a ficha e a imagem de conceito guardada, e não pode ser editada aqui. Veja [Compartilhar entre projetos](global-library.md#sharing).
