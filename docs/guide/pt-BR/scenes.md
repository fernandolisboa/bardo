---
id: scenes
title: Cenas e imagens
group: production
place: projects/scenes
tour: scenes
---

# Cenas e imagens

As cenas dividem a narração nos planos que o vídeo mostra, cada um com uma imagem. Elas ficam na etapa [Cenas](bardo:go/projects/scenes) de um projeto, e a etapa [Clipes](clips.md) as anima. [Mostre a etapa para mim](bardo:tour/scenes).

<a id="plan"></a>
## Planejar e desenhar

**Planejar cenas** faz o Claude transformar a narração num storyboard, a partir do **modelo de prompts de imagem** ([Modelos](templates.md)), das notas de estética do canal e do [estilo visual](#style) do vídeo. Primeiro ele escreve a história que as imagens contam da primeira à última, depois divide a narração em cenas pelas frases, de modo que cada cena continue a anterior, e escreve um prompt de imagem e um [prompt de movimento](clips.md#motion) para cada uma. Ele também lista as [pessoas que voltam](#cast) e os [lugares aos quais o vídeo volta](#same-place).

O plano cresce com a narração: um vídeo de um minuto tem cerca de dez cenas, um de dez minutos cerca de cem, e esse plano leva alguns minutos para o Claude escrever. O Bardo lê a resposta enquanto ela é escrita, então um plano longo não é abandonado enquanto ainda está chegando, e a estimativa sob **Planejar cenas** acompanha a duração da narração. Uma narração que pode render mais de cerca de 245 cenas (pouco mais de 16 minutos de frases curtas, mais quando elas são longas) não é planejada: o plano poderia não caber numa resposta, e uma resposta cortada é paga e perdida. A etapa avisa no lugar do botão; use uma narração mais curta ou divida o vídeo em mais de um.

Cancelar o plano em [Tarefas](jobs.md#cancel) para ele em instantes: o Bardo fecha o pedido em vez de esperar o fim da resposta, e [Custos](costs.md#spent) conta uma estimativa do que o Claude pode ter cobrado. Quando a resposta inteira chega bem na hora em que você cancela, ela está paga e fica guardada: **Planejar cenas** faz o plano a partir dela sem pedir de novo ao Claude. O mesmo vale para uma resposta que chegou e cujo plano o Bardo não conseguiu salvar. Enquanto um plano ou uma tarefa de imagens cancelada termina o que já tinha enviado, a etapa diz **Cancelada. Terminando o que já tinha sido enviado…** e não começa nada novo, para o mesmo trabalho não ser pago duas vezes; no caso de uma imagem sendo desenhada, isso pode levar o tempo dessa imagem.

**Gerar N imagens** então desenha, numa só tarefa e em ordem, cada cena que ainda não tem imagem: cada imagem é desenhada com [imagens anteriores do vídeo](#same-place) ao lado do prompt, para que as cenas pareçam um só vídeo. O Nano Banana desenha cada uma a 2K no formato do [vídeo](projects.md#format), 9:16 para um Short e 16:9 para um vídeo longo, e o Gemini cobra cada imagem; uma cena que o provedor recusa não para as outras, e tentar a tarefa de novo desenha só as cenas que ela ainda não desenhou: uma cena cuja imagem a tarefa já entregou não é desenhada de novo, mesmo que você tenha rejeitado ou substituído essa imagem depois, nem uma que você desenhou à parte depois que a tarefa começou. A estimativa sob o botão principal diz quanto a rodada custa. Planejar exige uma chave do Claude, e desenhar uma chave do Gemini ([Suas chaves de API](api-keys.md#providers)).

<a id="list"></a>
## As cenas

As cenas aparecem em ordem, cada uma com o tempo, a imagem e como está: **A gerar**, **Revisar**, **Falhou** ou pronta. Escolha uma cena para vê-la inteira; ↑ e ↓ percorrem as cenas, e Enter usa a nova imagem de uma cena em revisão.

<a id="scene"></a>
## Uma cena

A cena escolhida mostra o trecho da narração que ela cobre, a imagem e o **prompt de imagem**. **Editar prompt** muda o que o próximo desenho mostra (até 4.000 caracteres); uma cena cujo prompt você mudou ganha a marca **Editado**. **Detalhes da geração** diz qual modelo de IA desenhou a imagem, com quantos tokens e qual versão do modelo de prompts.

<a id="ad"></a>
## As cenas de um anúncio

As cenas de um [anúncio](brief.md) são planejadas a partir do modelo **Plano de cenas de anúncio** ([Modelos](templates.md)): uma cena por frase, de cerca de 2 a 4 segundos cada, a primeira mostrando o gancho e a última sendo o cartão final, sobre a chamada para ação. Cada cena de um anúncio tem um **tipo**:

| Tipo | O que mostra | Quem faz |
| --- | --- | --- |
| **Desenhada** | Uma imagem feita a partir do prompt da cena, como as cenas de qualquer vídeo | O modelo de imagem |
| **Produto em uso** | O produto sendo usado, a partir do prompt da cena e da foto do produto, [conferido por você](#product-check) | O modelo de imagem |
| **Foto do produto** | Uma das imagens do próprio produto, como ela é | O Bardo, de graça |
| **Cartão final** | A [última imagem](#end-card) do anúncio, da chamada para ação até depois da última palavra | O Bardo, de graça, a partir de uma imagem do produto, do logo e das suas linhas; um cartão de texto quando o produto não tem imagem |

Os tipos que um plano pode usar dependem das [imagens](products.md#pictures) do produto: uma foto do produto exige uma imagem, e o produto em uso exige uma **Foto**, porque uma **Tela** só é mostrada como ela é. Um produto sem imagem recebe cenas desenhadas e um [cartão final](#end-card) de texto. O Claude é avisado de quais tipos pode usar e, por número, de quais imagens o produto tem; um tipo que o produto não permite volta como cena desenhada, e uma foto do produto que cita uma imagem que o produto não tem mostra a primeira. Quando o produto tem imagem, o plano é pedido de modo que o produto esteja na tela quando a narração citar o nome dele, com pelo menos uma foto do produto.

O próprio Bardo compõe a foto do produto, no seu computador, assim que o plano é salvo: a imagem do produto, inteira, sobre um fundo liso numa cor tirada da imagem ([a aparência](#shot-look) você pode mudar), 1080 × 1920 num vídeo vertical e 1920 × 1080 num vídeo largo. A imagem é redimensionada para caber e nunca é esticada, cortada nem redesenhada, e fica onde nem o movimento do corte nem uma legenda a cobre. Compor não custa nada e não exige chave, e a imagem do produto nunca vai para um modelo de imagem ou de vídeo a partir de uma foto do produto. Uma imagem composta nunca é animada ([Clipes](clips.md#animate)), nunca serve de referência para desenhar outra cena, não é oferecida como cenário de uma tomada atuada e, no editor, [só recebe zoom](editor.md#motion).

**Tipo de cena**, na cena escolhida, muda o que ela é, entre os tipos que o produto permite; **Cartão final** só aparece na última cena. Uma cena que vira foto do produto é composta na hora, e **Imagem do produto** escolhe qual das imagens do produto ela mostra, numeradas como no produto. Quando essa imagem é pequena para o quadro, a cena avisa: ampliada, ela pode ficar sem nitidez. Uma cena que volta a ser **Desenhada** retoma o desenho que tinha; se nunca teve um, nada é desenhado até você pedir em **Gerar N imagens**, que mostra o custo antes. Quando o vídeo muda de formato, as imagens compostas deixam de servir e aparecem como **A compor**: **Compor fotos do produto** (ou **Compor imagem**, na cena) faz essas imagens de novo, de graça, e **Gerar N imagens** as faz junto com os desenhos. Uma cena cuja imagem foi removida do produto avisa e fica com a imagem que tem; quando essa imagem precisa ser feita de novo, a cena falha até você escolher outra imagem ou outro tipo para ela. Um render no outro formato (um anúncio longo publicado numa rede 9:16, ou o contrário) mostra a imagem composta inteira, com faixas.

A [estimativa](projects.md#mode) do vídeo conta as cenas compostas como gratuitas. Antes de o anúncio ter um plano, ele é estimado com uma cena a cada 3 segundos, das quais o cartão final é gratuito, e mais duas fotos do produto quando o produto tem imagem.

<a id="product-check"></a>
## O produto em uso

Uma cena de **Produto em uso** é desenhada com a primeira **Foto** do produto como referência, logo depois dos retratos das pessoas da cena e antes das outras tomadas do vídeo, junto com a aparência do produto (da ficha dele) e uma frase fixa: manter o formato, as cores, o rótulo e as proporções do produto exatamente como na foto. Uma **Tela** nunca vai para um modelo: um produto cujas imagens são todas telas não tem cena assim, e as cenas desenhadas mostram telas ilegíveis. Uma cena que virou **Produto em uso** com um desenho feito sem a foto aparece como **A gerar**, e **Gerar N imagens** a desenha de novo com a foto, depois de mostrar o custo.

Toda imagem do produto em uso espera a sua conferência: a cena mostra **Conferir produto**, e o inspetor a mostra ao lado de **O produto de verdade**. Compare formato, cores, rótulo e proporções:

- **Está igual** confere a imagem. Só então a cena é animada em [Clipes](clips.md#product-check), e só então o vídeo pode ser [renderizado](render.md#gates).
- **Desenhar de novo** desenha uma imagem nova, paga, com a estimativa; a nova espera a revisão e depois a própria conferência.
- **Usar a imagem real** torna a cena uma [foto do produto](#ad), composta a partir da imagem do próprio produto, de graça.

O Bardo não julga a semelhança: a conferência é sua, a olho. Uma imagem desenhada de novo e usada perde a conferência. Uma cena esperando conferência bloqueia só o próprio clipe e a renderização, nunca as cenas ao redor. Num produto do tipo *ideia*, a referência é a [imagem de conceito](products.md#concept) mantida, um desenho: o inspetor a chama de **Imagem de conceito**, pergunta se o produto está igual a ela e oferece **Usar a imagem de conceito no lugar**.

<a id="shot-look"></a>
## A aparência de uma foto do produto

Uma foto do produto (e o [cartão final](#end-card)) tem controles próprios abaixo de **Tipo de cena**. No cartão final, a imagem fica no espaço que o texto deixa, então **Posição** não aparece, e um cartão de texto tem só o fundo. Escolher um layout, uma posição ou um fundo compõe a imagem de novo, no seu computador, de graça; só desenhar um fundo novo é pago.

- **Layout**, quando a imagem oferece mais de um:
  - **Cartão**: a imagem com cantos arredondados. Toda imagem pode ser um cartão.
  - **Recortado**: uma **Foto** de fundo transparente (um PNG com transparência) fica em pé sobre o fundo, com uma sombra suave. É o que uma foto assim recebe de início. Uma foto sem nenhum pixel transparente aparece como cartão.
  - **Aparelho**: uma **Tela** dentro de um contorno simples desenhado pelo Bardo: um celular para uma tela alta, uma janela de navegador para uma larga ou quadrada, sem marca de fabricante. É o que uma tela recebe de início.
- **Posição**: **Esquerda**, **Centro** ou **Direita** dentro do espaço que o quadro deixa. As margens e a faixa da legenda ficam livres, e o produto continua na tela no zoom mais forte.
- **Fundo**:
  - **Da imagem**: um tom escuro ou claro da própria cor da imagem, como no início.
  - **Cor**: um dos tons da imagem, branco ou preto. Escolha uma segunda cor em **Degradê até** para um degradê entre as duas, e a direção: de cima para baixo, da esquerda para a direita ou diagonal. Escolher de novo a primeira cor em **Degradê até** deixa uma cor só.
  - **Desenhar um fundo** pede ao gerador de imagens uma superfície ou um cenário vazio no [estilo](#style) e no formato do vídeo, sem produto, texto ou logo. Ele não vê nenhuma imagem do produto e só fica sabendo que tipo de produto é. Custa uma imagem de cena: a estimativa fica ao lado do botão, conta para o [teto](projects.md#mode) do vídeo e aparece em Custos como **Fundo de foto do produto**. O fundo novo espera por você em **Novo fundo para revisar**: **Manter e usar** põe esse fundo atrás desta cena, **Descartar** apaga. Enquanto um espera, **Desenhar um fundo** espera também. Os fundos mantidos aparecem em **Fundos desenhados** para toda foto do produto do projeto e não custam mais nada; escolha um para mostrá-lo. **Descartar** abaixo de um fundo mantido o apaga, a não ser que uma foto do produto ainda o mostre.

Seja qual for a aparência, a imagem do produto só é redimensionada: nunca esticada, cortada ou redesenhada. Uma imagem composta antes de existir aparência aparece como **A compor** e é feita de novo, de graça, em **Compor fotos do produto**.

<a id="end-card"></a>
## O cartão final

A última cena de um anúncio é o cartão final, feito como uma [thumbnail](uploading.md#thumbnail) e nunca por um modelo: o fundo, a imagem do produto que a cena mostra (inteira, só redimensionada), o [logo](products.md#pictures) do produto quando ele tem um, e até três linhas desenhadas nas fontes do Bardo. Um produto sem imagem recebe um cartão de texto, com o logo se tiver. Compor não custa nada e não exige chave, e nada disso é enviado para lugar nenhum. O editor fica no inspetor da cena, abaixo de **Tipo de cena**:

- **Título**: começa como a chamada para ação do briefing; digite o seu, com até 40 caracteres.
- **Segunda linha**: começa como o link ou @ do briefing, mantido como você digitar; até 60 caracteres, e pode ficar vazia. **Salvar cartão final** salva as duas linhas.
- **Mostrar a oferta**: a linha de oferta só mostra a [oferta do briefing](brief.md), então uma oferta nova lá aparece no cartão na hora. Sem oferta no briefing, não há linha de oferta. O que você digita no cartão é seu: a [checagem de afirmações](script.md#claims) não lê.
- **Visual**: **Forte**, **Amarelo** ou **Faixa**, os visuais das thumbnails.
- **Texto**: **Acima da imagem** ou **Abaixo da imagem**. **Logo**: **No canto** ou **Junto ao texto**. Todo lugar fica longe dos botões e das legendas das redes, em 9:16 e em 16:9.

Toda mudança compõe o cartão de novo, de graça. Até lá, o inspetor diz **A imagem mostra um design anterior** e a cena mostra **A compor**: o vídeo não é [renderizado](render.md#gates) com um cartão antigo. O cartão é composto no formato em que o vídeo é cortado, e de novo quando ele muda.

No corte, o cartão aparece da primeira palavra da chamada para ação até 1,5 segundo depois da última palavra, e por pelo menos 2,5 segundos; a música continua por baixo, nenhuma legenda aparece sobre ele e ele [fica parado](editor.md#motion). Para desligar, faça da última cena uma cena desenhada ou uma foto do produto em **Tipo de cena**: o corte então termina com a narração. Voltar a cena para **Cartão final** traz o cartão de volta com o design que você salvou.

<a id="redraw"></a>
## Desenhar de novo

**Desenhar de novo** faz uma nova imagem a partir do prompt da cena, do lugar, das pessoas e das outras imagens do vídeo como estão agora. A nova imagem espera ao lado da atual: **Usar a nova imagem** a substitui, **Manter a atual** descarta a nova. Até você decidir, a cena mostra **Revisar** e a etapa diz quantas esperam.

<a id="cast"></a>
## Pessoas que voltam

Quando a mesma pessoa aparece em mais de uma cena (o chef, a detetive), o plano a lista uma vez com uma aparência fixa: sexo, idade, tom de pele, cabelo, porte e roupas. Antes de desenhar a primeira cena em que ela aparece, o Nano Banana desenha o **retrato de referência** dela, e toda cena com ela é desenhada com esse retrato e essa aparência, para que mantenha o mesmo rosto de cena para cena. O retrato é cobrado como uma imagem de cena, e a estimativa o conta.

Uma cena que mostra alguém que volta lista essa pessoa em **Pessoas nesta cena**, com o retrato. **Editar aparência** muda a descrição; o retrato deixa de corresponder a ela e sai, e a próxima cena desenhada com essa pessoa desenha um novo antes. **Desenhar retrato de novo** faz um novo retrato com a mesma aparência. Nos dois casos, as cenas já desenhadas mantêm a imagem, e o Bardo as marca: uma cena cuja imagem foi desenhada com um retrato mais antigo de alguém que aparece nela, ou sem o retrato dessa pessoa, mostra **Rosto antigo** e diz isso quando você a escolhe, e a etapa conta essas cenas em **Cenas com um rosto antigo**, também na etapa Clipes, para você ver isso antes de pagar para animar uma delas. **Desenhar todas de novo**, ali, desenha todas numa só tarefa, depois de mostrar quanto custa; cada imagem nova espera a revisão ao lado da atual. Enquanto uma imagem mostra um rosto antigo, as próximas cenas não são desenhadas a partir dela: só as imagens que batem com os retratos seguram as outras. Elas não contam como imagens que faltam, então **Gerar N imagens** não mexe nelas.

O Bardo não desenha uma cena sem o retrato de cada pessoa dela que volta no vídeo: a imagem seria paga e mostraria outro rosto. Quando um retrato não pode ser desenhado (o provedor recusa, por exemplo), a pessoa diz o motivo, as cenas que a mostram ficam sem desenhar e dizem que esperam o retrato, e as outras cenas são desenhadas normalmente; a falha da tarefa lista as cenas que esperam. Um retrato cujo arquivo não pode mais ser lido é deixado de lado do mesmo jeito: a pessoa volta a não ter retrato, e ele é desenhado da próxima vez como qualquer retrato que falta. Depois que um retrato de que a cena precisa falha, o Bardo não compra os outros retratos dessa cena na mesma tarefa. Desenhe o retrato de novo, editando antes a aparência se o provedor recusou, e depois desenhe as cenas, ou tente a tarefa de novo, que faz as duas coisas. Se preferir seguir sem retrato para uma pessoa, **Desenhar sem retrato**, oferecido para todas, faz as cenas dela serem desenhadas só pela descrição: a pessoa mostra **Sem retrato**, nenhum retrato é desenhado, enviado nem cobrado para ela, o rosto dela pode mudar de uma cena para outra, e as cenas que esperavam o retrato dela deixam de esperar. **Usar um retrato** desfaz essa escolha, e as imagens desenhadas sem retrato passam a mostrar **Rosto antigo**.

<a id="same-place"></a>
## O mesmo lugar de cena em cena

Quando mais de uma cena acontece no mesmo lugar (a cozinha, a sala de controle), o plano lista o lugar uma vez com uma única descrição fixa: o que ele é, a época, os objetos principais, a hora do dia, a luz e as cores. Uma cena que acontece ali o mostra em **Lugar**. Uma cena num lugar onde nenhuma outra acontece não tem lugar listado, e descreve o lugar no próprio prompt.

Toda imagem é desenhada com outras imagens do vídeo, que o Nano Banana recebe como referências ao lado do prompt:

- **Uma cena num lugar já desenhado** recebe a descrição do lugar, a primeira imagem desenhada ali e, quando a cena logo antes dela acontece no mesmo lugar, essa imagem também, para continuar a partir dela: a mesma sala, luz e hora do dia, vistas de outro ângulo.
- **A primeira cena de um lugar** recebe a descrição do lugar e a primeira imagem do vídeo, só para seguir o jeito como ela é desenhada (a técnica, o traço, o nível de detalhe). **Uma cena num lugar só dela** também recebe essa imagem.

A primeira imagem de todas não tem o que seguir, então ela define a cara do vídeo. Uma cena desenhada de novo segue as imagens que o vídeo tem naquele momento, inclusive a primeira cena, então desenhar uma cena de novo mantém a cara das outras. Uma imagem que espera a sua revisão ainda não é referência: aceite-a, e as cenas desenhadas depois passam a segui-la. As referências quase não custam nada a mais: uma imagem custa o mesmo com ou sem elas. Uma imagem grande vai como uma cópia menor (um JPEG do mesmo tamanho), que o Bardo guarda ao lado dela na pasta do projeto.

**Editar lugar** muda a descrição para todas as cenas do lugar; um lugar que você mudou ganha a marca **Editado**. As cenas já desenhadas mantêm a imagem: desenhe-as de novo para usar a nova descrição. As imagens desenhadas com a descrição antiga deixam de amarrar o lugar, então desenhe de novo a primeira cena do lugar, aceite a nova imagem, e as cenas desenhadas depois passam a segui-la. Um lugar não pode ser editado enquanto imagens estão sendo desenhadas. Se você reescrever o prompt de uma cena de modo que ele não cite mais o lugar (o nome em palavras inteiras, com ou sem "the"), a cena sai do lugar e é desenhada sem ele.

Um plano guarda no máximo oito lugares, os que têm mais cenas; uma cena em qualquer outro leva a descrição no próprio prompt. Cenas planejadas antes de existirem lugares não têm nenhum; ainda assim recebem a primeira imagem do vídeo, pelo estilo. **Planejar de novo** dá lugares a elas. Um [modelo](templates.md) de plano de cenas que você mesmo editou também recebe lugares, mas não as regras de storyboard do modelo padrão (uma história em ordem, ficando num lugar por várias cenas).

<a id="style"></a>
## Um estilo para o vídeo inteiro

O [estilo visual](projects.md#style) do vídeo é acrescentado pelo Bardo a cada imagem de cena e a cada retrato, sempre com as mesmas palavras, depois das pessoas e do lugar da cena. Com **Pelas notas do canal**, nada é acrescentado e o visual é o que os prompts das cenas disserem.

Trocar o estilo, no projeto ou no canal, deixa as imagens como estão. A etapa passa a mostrar **Cenas em outro estilo** com a quantidade:

- **Desenhar todas de novo** desenha todas elas no estilo atual, depois de mostrar quanto custa. O retrato de uma pessoa no estilo antigo é desenhado de novo antes, uma vez, e entra na estimativa; ele toma o lugar do antigo, então voltar ao estilo anterior o desenha mais uma vez, e a pessoa pode sair um pouco diferente das cenas já desenhadas. Cada imagem nova espera a revisão ao lado da atual, como qualquer cena desenhada de novo.
- Uma imagem em outro estilo nunca vai como referência, então não puxa as próximas de volta ao visual antigo. A primeira cena desenhada no estilo novo é desenhada só com o prompt, o lugar e as pessoas dela; as seguintes se guiam pelas imagens novas, mesmo enquanto elas esperam a revisão.
- Desenhar de novo uma cena só faz o mesmo para essa cena. As cenas desenhadas depois seguem a imagem nova dela.

Os prompts das cenas foram escritos para o estilo que o vídeo tinha quando foi planejado. Se eles citam outro visual (um plano feito pelas notas costuma citar), **Planejar de novo** os reescreve para o estilo novo; isso descarta as imagens, então faça antes de desenhar.

<a id="filter"></a>
## Todas ou pendentes

Acima das cenas, **Todas** mostra cada uma e **Pendentes** só as que têm algo por fazer: uma imagem para desenhar ou revisar (na etapa Clipes, um clipe para fazer ou revisar). Cada aba diz quantas cenas tem.

<a id="fit"></a>
## Depois de uma nova narração

Quando a narração é refeita, os tempos das cenas não batem mais e a etapa mostra **Desatualizadas**. Até as cenas baterem de novo com a narração, o Bardo não faz nenhuma imagem ou clipe para elas, porque sairia nos tempos de uma narração que não existe mais: os botões que pagam por isso ficam desligados e dizem por quê, e uma tarefa executada de novo pelo painel Tarefas para sem enviar nada novo. Um clipe que já tinha sido enviado ao provedor ainda é recebido e guardado.

Se a nova narração lê o mesmo texto em que as cenas foram planejadas (o roteiro não mudou), a etapa oferece **Ajustar à narração**. Isso mantém cada cena com seus prompts, pessoas, lugares, imagens e clipes, e lê de novo, na nova narração, quando cada uma começa e termina. Não custa nada. Uma cena que agora dura mais que o clipe segura o último quadro dele (um clipe feito [em câmera lenta](clips.md#stretch) antes desacelera mais, até onde o limite dele permite), e o corte no [editor](editor.md) recomeça do corte bruto nos novos tempos, como depois de qualquer nova narração.

As duas coisas esperam uma pela outra: enquanto uma narração está sendo feita, as cenas não podem ser planejadas, desenhadas nem animadas, e enquanto cenas, imagens ou clipes estão sendo feitos, uma nova narração espera por eles.

<a id="replan"></a>
## Planejar de novo

Quando o roteiro também mudou, a nova narração lê outro texto e as cenas não podem ser ajustadas a ela: **Planejar de novo** troca as cenas por novas. Se elas já têm imagens ou clipes, o Bardo pergunta antes, porque planejar de novo descarta esses arquivos. **Planejar de novo** também fica ali sempre que você quiser um novo storyboard para a mesma narração.

<a id="place"></a>
## Posicionar as cenas numa versão

Uma [versão em outro idioma](script.md#versions) começa com as cenas do original, ainda nos tempos do original. Depois que a narração dela fica pronta (gerada ou importada), a etapa mostra **Cenas para posicionar** e oferece **Posicionar cenas nesta narração**. Cada cena mantém a imagem, o clipe e os prompts e vai para o novo tempo da frase em que começava; o texto dela passa a ser as palavras da nova narração. As frases são encontradas comparando as palavras da adaptação com o que foi narrado, então editar o roteiro antes não atrapalha.

Posicionar também monta o corte da versão a partir do original: o enquadramento, o movimento e a transição de cada cena, as cenas que ele deixou de fora, o estilo e o destaque das legendas, a mixagem e o formato do quadro. Música e SFX vão para o mesmo ponto da narração (a música mantém início e fim nas palavras, um SFX a sua duração). As legendas vêm da nova narração. Cortes e aparas dentro de uma cena, imagens importadas colocadas à mão e textos de legenda editados não vêm junto; refaça-os no [editor](editor.md).

As cenas esticam ou encolhem com as suas frases. **Vale conferir** lista as cenas que agora passam do clipe (o clipe segura o último quadro), as que cresceram mais da metade ou encolheram mais de um terço e as que a versão não narra, que ficam fora do corte. Depois de uma nova narração, posicionar volta a ser oferecido, e até as cenas serem posicionadas nenhuma imagem ou clipe é feito para elas; **Planejar de novo** substitui as cenas como em qualquer projeto.
