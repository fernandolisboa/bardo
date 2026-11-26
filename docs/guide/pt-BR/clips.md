---
id: clips
title: Clipes
group: production
place: projects/clips
tour: clips
---

# Clipes

Um clipe anima a imagem de uma cena num vídeo curto. Os clipes são opcionais: uma cena sem clipe mostra a imagem parada no vídeo. Eles ficam na etapa [Clipes](bardo:go/projects/clips), que se abre quando uma cena tem imagem ([Cenas e imagens](scenes.md)). [Mostre a etapa para mim](bardo:tour/clips).

<a id="animate"></a>
## Animar as cenas

**Animar cenas sem clipe** envia ao provedor de vídeo, numa só tarefa, cada cena que tem imagem e não tem clipe; **Animar** numa cena faz só aquela. O [modo de produção](projects.md#mode) do vídeo diz quais cenas o primeiro botão envia: no **Gancho animado** ele vira **Animar o gancho** e envia só as cenas que começam nos primeiros 10 segundos, e no **Econômico** ele não aparece, já que todas as cenas ficam em imagens com movimento. **Animar** numa cena funciona em qualquer modo. Uma cena de anúncio que o Bardo compôs a partir de uma imagem do produto ([As cenas de um anúncio](scenes.md#ad)) nunca é animada, em nenhum modo: o corte mostra essa imagem como ela é. Cada clipe dura o tempo em que a cena é narrada, dentro das durações que o modelo aceita; num modelo de durações fixas, uma cena um pouco mais longa que uma delas leva esse clipe mais curto, [em câmera lenta](#stretch). Uma cena que falha não para as outras. Quando o pedido de uma cena falha do lado do provedor, o Bardo nunca a envia de novo por conta própria: nem quando a tarefa espera uma falha passageira passar e recomeça, nem quando o Bardo é fechado e aberto de novo. Só quando você mesmo tenta a tarefa de novo é que elas são enviadas, junto com as cenas que a tarefa ainda não tinha enviado. Uma cena cujo clipe a tarefa já entregou fica como está, mesmo que você tenha rejeitado ou substituído esse clipe depois, e uma cena que você animou à parte depois que a tarefa começou não é enviada de novo. Uma tarefa não pode ser tentada de novo enquanto outra anima uma de suas cenas: espere essa terminar. Enquanto as cenas estão desatualizadas depois de uma nova narração, nada é animado até elas serem ajustadas a ela ou planejadas de novo ([Cenas](scenes.md#fit)).

Os clipes são feitos pela Higgsfield, pelo Kling ou pelo Google, com a sua própria chave de cada um ([Suas chaves de API](api-keys.md#providers)), ou com os créditos do seu plano Higgsfield depois que você faz login nele ([O plano Higgsfield](api-keys.md#higgsfield-plan)). Cancelar uma rodada para a espera, não o provedor: um clipe já enviado ainda pode ficar pronto e ser cobrado. Um clipe entra em [Custos](costs.md), e conta para o [teto](projects.md#mode) do vídeo, a partir do momento em que o provedor o aceita, mesmo que o Bardo nunca o baixe. Um clipe que o provedor informa que falhou ou que recusou não é cobrado e não deixa custo; um clipe de que o provedor já não sabe dar notícia (um pedido que ele esqueceu, por exemplo) mantém o custo, porque pode ter sido cobrado. Se um clipe pronto não puder ser baixado (o link expirou, por exemplo), tentar a tarefa de novo busca o mesmo clipe em vez de pagar por outro; **Animar** na cena pede um clipe novo, e paga por ele. O Google não tem como dizer ao Bardo se um pedido cuja resposta nunca chegou (a conexão caiu, ou o Bardo foi fechado no meio do pedido) iniciou um clipe. O Bardo não envia esse pedido de novo sozinho: a cena falha dizendo que o clipe pode ter sido feito, o preço dele entra em Custos porque o Google pode ter cobrado, e tentar a tarefa de novo pede um clipe novo. Confira o seu uso no Google quando o valor importar.

<a id="motion"></a>
## Movimento

O **prompt de movimento** diz como a imagem se move: um movimento de câmera e no máximo uma ação simples do assunto. O Claude escreve um para cada cena quando [planeja as cenas](scenes.md#plan); cenas planejadas antes disso usam o prompt de imagem, marcadas como **Igual ao prompt de imagem**. **Editar movimento** o muda para o próximo clipe da cena, e salvá-lo vazio volta ao do Claude.

<a id="model"></a>
## Modelo de vídeo

Cada cena usa o modelo padrão do vídeo, a menos que você escolha outro para ela em **Modelo de vídeo**: o do [modo de produção](projects.md#mode) (Kling 2.6 ou 3.0) ou, no **Tudo animado, modelo do canal**, o do canal, definido em [Canais](bardo:go/channels). Escolha em passos, como em Canais: **Provedor**, **Modelo**, **Versão** e **Variante** ([Escolher um modelo](channels.md#model-picker)); a primeira opção de **Provedor** volta ao padrão do vídeo. Os modelos mudam no visual, nas durações que aceitam e no preço. Quando um provedor descontinua um modelo, como o Google fez com o Veo 3.1, os canais e as cenas salvos com ele passam a usar o modelo que o provedor oferece no lugar, aqui o Gemini Omni Flash na mesma resolução; a etapa ou a cena diz o nome dos dois modelos, e o preço embaixo da cena é o do modelo novo. Se o modelo de uma cena deixou de ser oferecido e nada ficou no lugar, a cena avisa; escolha outro.

<a id="review"></a>
## Revisar um clipe

Um novo clipe espera ao lado do atual como **Novo clipe para revisar**. **Reproduza-o** primeiro (**Reproduzir**), depois **Usar o novo clipe** ou **Descartar**. O clipe de uma cena mostra a duração e o modelo; **Usar a imagem parada** tira o clipe e volta à imagem. Quando a imagem da cena muda depois que o clipe foi feito, o clipe fica marcado como **Imagem mudou**.

<a id="product-check"></a>
## O produto em uso

Uma cena do [produto em uso](scenes.md#product-check) só é animada depois que você conferiu o produto na imagem dela; até lá ela mostra **Conferir produto** e **Animar cenas sem clipe** a deixa de fora. O prompt de movimento dela começa com uma frase fixa: o produto fica rígido e sem mudanças, e só a câmera ou a pessoa se mexe. O clipe dela espera uma conferência própria: **Reproduza** e observe o produto, depois **Continua igual**. Até você conferir, o corte mostra a imagem conferida no lugar do clipe. Um clipe novo usado na cena precisa ser conferido de novo.

<a id="stretch"></a>
## Clipes em câmera lenta

Um modelo de durações fixas (o Kling 2.6 faz 5 ou 10 segundos) raramente tem a duração da cena. Em vez de pagar pelo clipe mais longo e cortar o resto, o Bardo pede o mais curto e o reproduz mais devagar, para terminar junto com a cena: por padrão, uma cena de 6,5 segundos paga 5 segundos, não 10. Nesse caso, sob **Próximo clipe** aparece **Toca em câmera lenta para caber na cena**, e o clipe ganha a marca **Em câmera lenta**; o inspetor do editor mostra a duração com que ele foi feito e a duração em que toca.

O Bardo mescla os quadros intermediários em vez de repeti-los, então um clipe em câmera lenta não engasga. Por padrão um clipe estica em até 40%, o que o reproduz a cerca de 70% da velocidade ou mais rápido; uma cena longa demais para isso leva a duração seguinte e toca na velocidade normal, e uma cena mais longa que todas as durações leva a maior, em câmera lenta até onde o limite deixa. **Reproduzir** numa cena abre o arquivo do clipe como ele foi feito; a prévia do editor e o render o mostram em câmera lenta. O canal define o limite, ou desliga a câmera lenta, em [Duração dos clipes](channels.md#clip-length). Um clipe guarda o limite com que foi feito: mudar o canal muda os próximos clipes, nunca os que você já tem. Modelos que aceitam qualquer duração dentro de uma faixa fazem cada clipe com a duração da cena e nunca ficam em câmera lenta.

<a id="cost"></a>
## Quanto custa um clipe

Sob o modelo, **Próximo clipe** diz quanto vai durar o próximo clipe da cena, se ele vai tocar em câmera lenta e quanto ele custa pela tarifa do provedor; a estimativa sob **Animar cenas sem clipe** soma a rodada. O provedor cobra cada segundo. Um modelo sem tarifa em [Custos](bardo:go/costs) avisa, e você pode cadastrar o preço dele lá.
