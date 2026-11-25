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

**Animar cenas sem clipe** envia ao provedor de vídeo, numa só tarefa, cada cena que tem imagem e não tem clipe; **Animar** numa cena faz só aquela. O [modo de produção](projects.md#mode) do vídeo diz quais cenas o primeiro botão envia: no **Gancho animado** ele vira **Animar o gancho** e envia só as cenas que começam nos primeiros 10 segundos, e no **Econômico** ele não aparece, já que todas as cenas ficam em imagens com movimento. **Animar** numa cena funciona em qualquer modo. Cada clipe dura o tempo em que a cena é narrada, dentro das durações que o modelo aceita. Uma cena que falha não para as outras. Quando o pedido de uma cena falha do lado do provedor, o Bardo nunca a envia de novo por conta própria: nem quando a tarefa espera uma falha passageira passar e recomeça, nem quando o Bardo é fechado e aberto de novo. Só quando você mesmo tenta a tarefa de novo é que elas são enviadas, junto com as cenas que a tarefa ainda não tinha enviado. Uma cena cujo clipe a tarefa já entregou fica como está, mesmo que você tenha rejeitado ou substituído esse clipe depois, e uma cena que você animou à parte depois que a tarefa começou não é enviada de novo. Uma tarefa não pode ser tentada de novo enquanto outra anima uma de suas cenas: espere essa terminar.

Os clipes são feitos pela Higgsfield, pelo Kling ou pelo Google, com a sua própria chave de cada um ([Suas chaves de API](api-keys.md#providers)), ou com os créditos do seu plano Higgsfield depois que você faz login nele ([O plano Higgsfield](api-keys.md#higgsfield-plan)). Cancelar uma rodada para a espera, não o provedor: um clipe já enviado ainda pode ficar pronto e ser cobrado. Um clipe entra em [Custos](costs.md), e conta para o [teto](projects.md#mode) do vídeo, a partir do momento em que o provedor o aceita, mesmo que o Bardo nunca o baixe. Um clipe que o provedor informa que falhou ou que recusou não é cobrado e não deixa custo; um clipe de que o provedor já não sabe dar notícia (um pedido que ele esqueceu, por exemplo) mantém o custo, porque pode ter sido cobrado. Se um clipe pronto não puder ser baixado (o link expirou, por exemplo), tentar a tarefa de novo busca o mesmo clipe em vez de pagar por outro; **Animar** na cena pede um clipe novo, e paga por ele. O Google não tem como dizer ao Bardo se um pedido cuja resposta nunca chegou (a conexão caiu, ou o Bardo foi fechado no meio do pedido) iniciou um clipe. O Bardo não envia esse pedido de novo sozinho: a cena falha dizendo que o clipe pode ter sido feito, o preço dele entra em Custos porque o Google pode ter cobrado, e tentar a tarefa de novo pede um clipe novo. Confira o seu uso no Google quando o valor importar.

<a id="motion"></a>
## Movimento

O **prompt de movimento** diz como a imagem se move: um movimento de câmera e no máximo uma ação simples do assunto. O Claude escreve um para cada cena quando [planeja as cenas](scenes.md#plan); cenas planejadas antes disso usam o prompt de imagem, marcadas como **Igual ao prompt de imagem**. **Editar movimento** o muda para o próximo clipe da cena, e salvá-lo vazio volta ao do Claude.

<a id="model"></a>
## Modelo de vídeo

Cada cena usa o modelo padrão do vídeo, a menos que você escolha outro para ela em **Modelo de vídeo**: o do [modo de produção](projects.md#mode) (Kling 2.6 ou 3.0) ou, no **Tudo animado, modelo do canal**, o do canal, definido em [Canais](bardo:go/channels). Os modelos mudam no visual, nas durações que aceitam e no preço. Se o modelo de uma cena deixou de ser oferecido, a cena avisa; escolha outro.

<a id="review"></a>
## Revisar um clipe

Um novo clipe espera ao lado do atual como **Novo clipe para revisar**. **Reproduza-o** primeiro (**Reproduzir**), depois **Usar o novo clipe** ou **Descartar**. O clipe de uma cena mostra a duração e o modelo; **Usar a imagem parada** tira o clipe e volta à imagem. Quando a imagem da cena muda depois que o clipe foi feito, o clipe fica marcado como **Imagem mudou**.

<a id="cost"></a>
## Quanto custa um clipe

Sob o modelo, **Próximo clipe** diz quanto vai durar o próximo clipe da cena e quanto ele custa pela tarifa do provedor; a estimativa sob **Animar cenas sem clipe** soma a rodada. O provedor cobra cada segundo. Um modelo sem tarifa em [Custos](bardo:go/costs) avisa, e você pode cadastrar o preço dele lá.
