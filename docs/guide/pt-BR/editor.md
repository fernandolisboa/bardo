---
id: editor
title: O editor
group: editing
place: projects/edit
tour: editor
---

# O editor

É no editor que as cenas e a narração viram um corte: você apara, mixa o som, legenda e enquadra, e depois manda para o render. Ele abre sobre a janela inteira a partir da etapa [Edição](bardo:go/projects/edit) de um projeto, quando as cenas estão planejadas e narradas, e **Projetos**, no canto superior esquerdo, leva você de volta. O corte se salva sozinho a cada edição. [Mostre o editor para mim](bardo:tour/editor), ou [a parte 2 do tour](bardo:tour/editor-more) para a mixagem, as legendas, o enquadramento, o movimento nas imagens, os clipes interpretados e as sugestões de corte.

F1 abre este guia sobre o editor, e **Voltar ao editor** devolve você a ele como estava. Esc fecha um tour; o botão **Tour do editor** na barra de cima (Shift+F1) volta ao passo em que ele fechou.

<a id="preview"></a>
## Prévia e reprodução

A prévia, no meio, toca o corte a partir de cópias leves das suas imagens, clipes e narração (**Proxy**), então começa na hora; o render usa os arquivos completos. O Bardo gera essas cópias em segundo plano quando o editor abre; dá para editar enquanto isso, e a prévia toca quando elas ficam prontas. **Espaço** toca e pausa, **←** e **→** andam um quadro, e **Home** e **End** vão para o início e o fim.

Um clipe cujo arquivo sumiu, ou cuja cópia falhou, aparece em vermelho com o motivo acima da linha do tempo; **Tentar o proxy de novo** gera a cópia outra vez.

<a id="timeline"></a>
## Linha do tempo e faixas

O corte corre da esquerda para a direita em cinco faixas:

| Faixa | Guarda |
| --- | --- |
| CC | As legendas, seguindo as palavras da narração |
| V1 | O vídeo: um clipe por cena, mais os vídeos que você adicionar |
| A1 | A narração, com a forma de onda e as palavras |
| A2 | A música |
| A3 | Os efeitos sonoros |

Clique na linha do tempo para mover o cursor de reprodução, e clique num item para selecioná-lo: o cursor vai até ele e o inspetor, à direita, mostra as propriedades. **Esc** limpa a seleção. Ctrl e a roda do mouse dão zoom, ou use − e + no fim da barra de ferramentas.

O painel à esquerda lista as **Cenas** do projeto (clique numa para ir ao clipe dela) e a **Mídia**: **Importar arquivos…** traz arquivos de vídeo e áudio do seu computador, e os botões de cada arquivo o colocam numa faixa, no cursor de reprodução. A música gerada na etapa Roteiro aparece aqui depois que você a usa ([Música a partir do prompt](script.md#generated-music)), assim como cada clipe interpretado que você usa na etapa Captura ([Usar um clipe interpretado](capture.md#use)), cujo **Pôr no corte** o coloca em sincronia com a tomada ([Clipes interpretados](#performed)).

<a id="cuts"></a>
## Cortar

- **Dividir (S)** corta o item selecionado no cursor de reprodução, ou o clipe sob o cursor quando nada está selecionado.
- **Aparar**: arraste qualquer ponta de um item, ou aperte **[** ou **]** para levar o início ou o fim até o cursor.
- **Reordenar**: arraste um clipe para entre outros dois, ou use **Alt+←** e **Alt+→**. Nas faixas de áudio, as mesmas teclas movem o trecho selecionado um quadro.
- **Delete** (ou **Remover do corte**, no inspetor) tira a seleção do corte.

A faixa de vídeo fecha os vãos: aparar ou remover um clipe move os clipes seguintes. Os trechos de áudio se movem livremente e param nos vizinhos, e remover um deixa silêncio.

<a id="transitions"></a>
## Transições

Um corte vai direto de um clipe para o seguinte. Uma transição mistura os dois por um instante em volta do corte:

| Transição | O que faz |
| --- | --- |
| Fusão | O próximo clipe aparece aos poucos sobre o anterior |
| Pelo preto, Pelo branco | Desce para o preto (ou sobe para o branco) e entra no próximo clipe |
| Cortina ← → | Uma borda atravessa a tela e revela o próximo clipe |
| Deslizar ← → ↑ ↓ | O próximo clipe empurra o anterior para fora do quadro |
| Zoom | O clipe anterior se aproxima e dá lugar ao próximo |

**Transição (T)**, na barra de ferramentas, põe a transição padrão do canal (definida no [canal](channels.md#defaults)) no corte mais perto do cursor e seleciona o clipe depois dele; um corte que já tem transição fica com ela. Com um clipe selecionado, a **Transição de entrada** do inspetor escolhe como a imagem entra nele: **Corte** ou uma das transições, a **Duração** (de 0,1 a 2 s) e **Usar em todos os cortes** para aplicar a mesma escolha em todos. A transição aparece em V1 como uma faixa sobre o corte; clique nela para selecionar o clipe em que ela entra.

A transição fica centrada no corte: o clipe anterior continua por metade dela e o seguinte começa metade dela antes. O corte não sai do lugar, então o vídeo continua com a mesma duração e sincronizado com a narração. Onde o arquivo de um clipe não tem imagem além da borda, o quadro da borda fica parado durante essa metade. Uma transição nunca dura mais que os clipes vizinhos: ao lado de um clipe curto ela toca mais curta, e o inspetor diz por quanto tempo. O primeiro clipe abre o vídeo e não tem transição de entrada. Dividir um clipe deixa um corte simples entre as metades, e a transição acompanha o clipe quando você reordena.

A prévia e o render desenham as transições do mesmo jeito.

<a id="snapping"></a>
## Encaixar nas palavras

Com **Encaixar nas palavras** ligado, cortes, aparas e bordas de legenda caem entre as palavras da narração quando chegam perto, então um corte nunca come uma palavra; uma linha fina marca a palavra enquanto você arrasta. Segure **Alt** enquanto arrasta para posicionar livremente, ou desligue a chave.

<a id="retention"></a>
## Retenção sobre o corte

Quando o vídeo do projeto está no YouTube, a conta do YouTube do canal está conectada em [Contas](bardo:go/accounts) e uma [sincronização de métricas](performance-metrics.md#sync) já leu a retenção do público (o YouTube a informa 2 a 3 dias depois de o vídeo subir), **Retenção** na barra da linha do tempo desenha a curva numa faixa abaixo da régua, nos tempos do próprio corte. As quedas mais íngremes, onde o público sai pelo menos duas vezes mais rápido que a média daquele vídeo, ficam sombreadas e numeradas, no máximo as cinco maiores. A lista de quedas toma o lugar do inspetor: cada uma mostra o tempo, quanto do público sai ali, a cena exibida e a frase falada. Clique numa queda, na faixa ou na lista, para levar o cursor de reprodução até ela. Ligar **Retenção** esconde as sugestões de corte, e vice-versa. Enquanto o projeto não tem uma curva, **Retenção** fica acinzentado; passe o mouse sobre ele para ver o motivo.

A curva só se alinha com o corte enquanto ele toca como tocava quando o vídeo foi renderizado para o YouTube. Mixagem, legendas, enquadramento e movimento nas imagens fixas não mudam isso; uma apara, um movimento ou uma nova narração mudam. Aí a faixa fica vazia e a lista diz que o corte mudou, com o tempo de cada queda no vídeo publicado. Desfaça até voltar ao corte renderizado e a curva se alinha de novo. Nada das quedas fica guardado: elas são calculadas a partir da curva a cada vez.

<a id="undo"></a>
## Desfazer e refazer

Toda edição, inclusive de transições, mixagem, legendas, enquadramento e movimento, se desfaz com **Ctrl+Z** e se refaz com **Ctrl+Y** (ou **Ctrl+Shift+Z**), ou pelas setas no topo. O histórico vale enquanto o editor está aberto. Se as cenas ou a narração mudarem depois que você editou, o editor avisa e o corte recomeça do corte bruto.

<a id="mix"></a>
## A mixagem

Clique no cabeçalho de uma faixa de áudio para escolhê-la: o inspetor mostra o **Volume** (de −30 a +12 dB), **Mudo** e **Solo**. **M** e **S** no cabeçalho fazem o mesmo. Uma faixa que não vai tocar, muda ou deixada de fora pelo solo de outra, fica com o nome apagado.

Selecione um trecho de áudio para ajustar os **Fades**: entrada gradual no início e saída gradual no fim, desenhadas como rampas sobre o trecho.

<a id="ducking"></a>
## Atenuação

**Atenuar música sob a narração** abaixa a música enquanto a narração fala e a devolve nas pausas, seguindo as palavras. Escolha a faixa de música (A2) para definir **Atenuar em**, de 1 a 30 dB; o cabeçalho da faixa mostra a profundidade enquanto a atenuação está ligada.

<a id="captions"></a>
## Legendas

As legendas são feitas das palavras da narração e ficam na faixa CC. Selecione uma para editá-la no inspetor:

- **Texto**: Enter aplica; o tempo de exibição não muda.
- **Entrada** e **Saída**: arraste a legenda, ou qualquer ponta dela, na faixa; as bordas encaixam nas palavras.
- **Estilo**: escolha um dos estilos de legenda. O estilo vale para todas as legendas do projeto.
- **Palavra falada**: a cor em que acende a palavra sendo dita, enquanto o resto da linha fica branco, como na maioria dos Shorts; **Desligado** mostra cada linha na cor do próprio estilo. Só a cor muda, então a linha nunca se mexe nem quebra diferente. Cada palavra acende quando o narrador começa a dizê-la. Se você reescrever uma legenda com mais ou menos palavras do que as faladas, o tempo dela é dividido entre as novas palavras pelo tamanho de cada uma; corrigir uma palavra mantém o tempo. Vale para todas as legendas do projeto.

**Remover do corte** tira uma legenda.

**Mostrar legendas** liga ou desliga todas na prévia e no render; a [etapa Render](render.md#review) avisa quando estão desligadas.

<a id="framing"></a>
## Enquadramento: 16:9 ou 9:16

A chave sobre a prévia define o formato do quadro do corte: 16:9 (horizontal) ou 9:16 (vertical). Um corte novo começa no formato do [vídeo](projects.md#format): 9:16 para um Short, 16:9 para um vídeo longo. Trocar é uma edição que dá para desfazer.

No 9:16, um clipe cuja imagem é 16:9 é recortado dela. Selecione um clipe e deixe o cursor de reprodução nele: a prévia mostra a imagem inteira, escurecida fora da janela 9:16. Arraste a janela para escolher o que fica à vista, ou use o **Enquadramento** do inspetor: **Ajustar** mostra a imagem inteira com faixas em cima e embaixo, **Preencher** recorta no centro, e mover a janela deixa **Personalizado**. Um render 9:16 para uma rede também passa pela janela de recorte de cada clipe; um render 16:9 mostra cada imagem inteira, uma em pé com faixas nas laterais.

<a id="motion"></a>
## Movimento nas imagens

Uma imagem parada não precisa ficar parada. O **Movimento** dela move a vista sobre a imagem enquanto ela aparece, o efeito conhecido como Ken Burns, para um vídeo feito quase só de imagens não virar um slideshow:

| Movimento | O que faz |
| --- | --- |
| Aproximar | Da imagem inteira para perto do meio dela |
| Afastar | Do meio de volta para a imagem inteira |
| Pan ← → ↑ ↓ | Mais perto, a vista atravessa a imagem de uma borda à outra |

Selecione uma imagem e escolha o movimento dela no inspetor, ou **Nenhum** para deixá-la parada, e a **Intensidade**: **Sutil** (8% mais perto), **Média** (15%) ou **Forte** (25%). **Usar em todas as imagens** põe o movimento da imagem selecionada em todas (com **Nenhum**, tira o movimento de todas), e **Variar em todas as imagens** alterna zooms e pans na intensidade da imagem selecionada, para nenhuma imagem se mover como a anterior. Uma imagem sem movimento mostra a intensidade do canal, a que um movimento escolhido recebe. Um corte novo começa com os movimentos do canal, variados a menos que você mude no [canal](channels.md#defaults).

O movimento acontece sobre a imagem como ela está enquadrada: a janela 9:16 num corte vertical, a imagem inteira num 16:9. Onde a imagem aparece com faixas (**Ajustar** em 9:16, ou uma imagem alta em 16:9), as faixas se movem junto. Ele dura todo o tempo do clipe na tela, transições incluídas, então a imagem nunca para enquanto aparece. Dividir uma imagem dá o movimento inteiro a cada pedaço. Clipes têm movimento próprio, então o movimento vale só para imagens: uma cena que ganha um clipe guarda o movimento da imagem sem tocá-lo, e volta a tocá-lo se a cena mostrar a imagem de novo.

A prévia e o render desenham o movimento do mesmo jeito, suave a uma fração de pixel; uma imagem em movimento leva um pouco mais para renderizar do que uma parada.

<a id="performed"></a>
## Clipes interpretados

Um [clipe interpretado](capture.md#use) traz a sua própria atuação de uma tomada, então só funciona onde o corte toca a narração dessa tomada. Na aba **Mídia**, cada um traz a sua tomada ("Tomada 2"), e **Pôr no corte** o coloca na V1 onde o corte toca o início da tomada, cobrindo o que estiver ali pelo comprimento do clipe. Quando o corte deixou esse trecho da narração de fora, o Bardo avisa e não coloca nada: ponha a narração de volta na A1 primeiro.

**Usar clipes interpretados num novo corte bruto**, no alto da aba **Mídia** quando a narração tem clipes interpretados, faz cada novo corte bruto (uma narração nova, ou o corte recomeçado) tocar o clipe interpretado mais recente de cada tomada em sincronia, com as imagens das cenas entre eles. O corte que você tem agora fica como está.

Selecione um clipe interpretado e o inspetor diz se ele toca junto da narração: **Em sincronia com a narração**, quanto está adiantado ou atrasado (ao centésimo de segundo, para um quadro aparecer), ou que a narração dele foi cortada. Um corte, uma movimentação ou uma reordenação antes dele o desloca. **Voltar à sincronia** move o clipe, e o corte depois dele, alongando ou encurtando o clipe anterior; quando esse clipe não tem espaço para a diferença, apare os clipes anteriores à mão. Um clipe fora de sincronia também avisa na V1.

**Cobrir com a imagem de uma cena** mostra uma cena sobre parte da atuação (B-roll): escolha a **Cena** (a que está sob o cursor de reprodução, de início) e a **Duração** (0,5 a 10 s), depois **Cobrir a partir do cursor**. A cobertura para onde o clipe termina, e o resto da tomada continua em sincronia depois dela. Desfazer traz a tomada de volta inteira.

Num corte 9:16, um clipe cujo modelo mantém o seu enquadramento começa com a janela de recorte no seu rosto, como ele foi achado na análise da tomada; um clipe de controle de movimento começa centralizado, como as outras filmagens. Um clipe interpretado de uma narração que o projeto não usa mais é filmagem comum: sem tomada, sem sincronia. Uma versão em outro idioma mostra as imagens das cenas onde este corte toca clipes interpretados, já que eles falam este idioma.

<a id="suggestions"></a>
## Sugestões de corte

**Sugestões de corte da IA** mostra onde um corte cairia bem. **Sugerir cortes** manda os pontos onde uma frase termina, o narrador pausa ou a cena muda para o motor de decisão (TypeSafe, com a sua chave), que pontua cada um; a estimativa ao lado do botão diz quanto custa. Nada muda até você aceitar.

Cada sugestão é um marcador na régua com a nota. Clique num marcador para ver o motivo (**Fim de frase**, uma pausa, **Troca de cena**, **Mudança de assunto**), e então **Aceitar (A)** para dividir ali ou **Rejeitar (R)** para descartar; **Tab** vai para a próxima. Enquanto as sugestões aparecem, a lista toma o lugar do inspetor: **Aceitar todas acima de** uma nota, **Mostrar a partir de** para esconder as fracas, e **Desfazer** em qualquer linha já decidida. **Sugerir de novo** dá nota de novo aos pontos sem corte e substitui a lista.

<a id="render"></a>
## Ir para o render

**Revisar e renderizar**, no canto superior direito, fecha o editor e abre a [etapa Render](render.md) do projeto, onde o corte é verificado e renderizado para cada rede. O botão aparece quando o corte tem alguma coisa.
