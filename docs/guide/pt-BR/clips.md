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

**Animar cenas sem clipe** envia ao provedor de vídeo, numa só tarefa, cada cena que tem imagem e não tem clipe; **Animar** numa cena faz só aquela. Cada clipe dura o tempo em que a cena é narrada, dentro das durações que o modelo aceita. Uma cena que falha não para as outras, e tentar a tarefa de novo envia só as cenas ainda sem clipe.

Os clipes são feitos pela Higgsfield ou pelo Google, com a sua própria chave de cada um ([Suas chaves de API](api-keys.md#providers)). Cancelar uma rodada para a espera, não o provedor: um clipe já enviado ainda pode ficar pronto e ser cobrado.

<a id="motion"></a>
## Movimento

O **prompt de movimento** diz como a imagem se move: o assunto, a ação, a câmera. Até você escrever um, a cena usa o prompt de imagem, marcada como **Igual ao prompt de imagem**. **Editar movimento** o muda para o próximo clipe da cena.

<a id="model"></a>
## Modelo de vídeo

Cada cena usa o modelo de vídeo do canal, definido em [Canais](bardo:go/channels), a menos que você escolha outro para ela em **Modelo de vídeo**. Os modelos mudam no visual, nas durações que aceitam e no preço. Se o modelo de uma cena deixou de ser oferecido, a cena avisa; escolha outro.

<a id="review"></a>
## Revisar um clipe

Um novo clipe espera ao lado do atual como **Novo clipe para revisar**. **Toque** primeiro, depois **Usar o novo clipe** ou **Descartar**. O clipe de uma cena mostra a duração e o modelo; **Usar a imagem parada** tira o clipe e volta à imagem. Quando a imagem da cena muda depois que o clipe foi feito, o clipe fica marcado como **Imagem mudou**.

<a id="cost"></a>
## Quanto custa um clipe

Sob o modelo, **Próximo clipe** diz quanto vai durar o próximo clipe da cena e quanto ele custa pela tarifa do provedor; a estimativa sob **Animar cenas sem clipe** soma a rodada. O provedor cobra cada segundo. Um modelo sem tarifa em [Custos](bardo:go/costs) avisa, e você pode cadastrar o preço dele lá.
