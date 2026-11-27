---
id: templates
title: Modelos
group: production
place: templates
tour: templates
---

# Modelos

Os modelos são os prompts que o Bardo envia à IA. Mude um para mudar como o Bardo escreve roteiros, planeja cenas, redige prompts de música ou prepara publicações, para todos os canais. Eles ficam em [Modelos](bardo:go/templates). [Mostre a tela para mim](bardo:tour/templates).

<a id="kinds"></a>
## Tipos de modelo

Há um modelo para cada tipo de geração:

- **Roteiro**: o roteiro do vídeo ([Roteiro](script.md)). O roteiro de um anúncio vem do modelo Roteiro de anúncio.
- **Prompts de imagem**: o plano de cenas e o prompt de imagem de cada cena ([Cenas e imagens](scenes.md#plan)).
- **Prompt de música**: o prompt para a sua ferramenta de música ([Roteiro](script.md#music)).
- **Metadados**: o título, a descrição e as tags de cada rede, na etapa Publicação. Os de um anúncio vêm do modelo Metadados de anúncio.
- **Miniatura**: o texto da miniatura do YouTube de um vídeo longo e o prompt do fundo dela ([Miniatura](uploading.md#thumbnail)).
- **Capítulos**: os capítulos do YouTube de um vídeo longo, a partir das frases da narração ([Capítulos](uploading.md#chapters)).
- **Versão localizada**: o roteiro de uma versão, adaptado frase por frase para outro idioma ou país ([Versões em outros idiomas](script.md#versions)). Além dos dados do canal de destino, ele lê `{{source_language}}`, `{{source_country}}`, o título original e as frases numeradas.
- **Prompt de aparência**: o prompt de imagem da aparência desenhada de uma persona, a partir da sua descrição ([A aparência](personas.md#look)). Ele lê `{{persona}}` e `{{look_description}}`, e precisa continuar recusando pessoas reais.
- **Prompt da tomada**: o prompt que um modelo de tomada recebe para uma tomada de uma captura ([O prompt da tomada](capture.md#take-prompt)). Ele lê `{{look}}`, `{{set}}`, `{{words}}` e `{{instruction}}`: o que o modelo faz com o modo da tomada, que o prompt deve manter.
- **Ângulos de anúncio**: os ângulos de anúncio propostos para um produto em um canal ([Ângulos de anúncio](products.md#angles)). Além dos dados do canal, ele lê `{{product}}`, `{{product_audience}}`, `{{product_claims}}` (as afirmações confirmadas, numeradas), `{{product_notes}}`, `{{ad_frameworks}}`, `{{angles_to_avoid}}` e `{{angle_count}}`. Depois das instruções o Bardo sempre acrescenta as regras de todo anúncio, que nenhuma edição tira: dizer só o que as afirmações numeradas dizem; não inventar depoimento, avaliação, nota, número de clientes, prêmio ou menção na imprensa; não dizer prazo nem escassez que as notas do anunciante não dizem; nunca apresentar uma pessoa desenhada como cliente, médico ou especialista de verdade; nunca mostrar o logo de outra marca nem copiar o anúncio de um concorrente.
- **Roteiro de anúncio**: o roteiro de um anúncio ([Roteiro](script.md#write)). Além dos dados do canal, do tema e de `{{target_length}}` (2 segundos a menos que a duração do anúncio), ele lê `{{product}}` (nome, tipo e resumo), `{{product_concept}}` (se o produto existe ou é um conceito), `{{ad_segment}}` (com quem o anúncio fala), `{{ad_hook}}` (as primeiras palavras do ângulo), `{{ad_framework}}` (o formato do anúncio e os passos dele), `{{brief_claims}}` (as afirmações da etapa Briefing, numeradas), `{{offer}}`, `{{call_to_action}}`, `{{must_say}}` e `{{never_say}}`. Depois das instruções dele, o Bardo sempre acrescenta as regras de todo anúncio, como nos ângulos de anúncio, e, num produto do tipo *ideia*, as regras de um conceito: falar dele como uma ideia, nunca dizer nem dar a entender que dá para comprar, encomendar, baixar ou usar hoje, e não dizer preço nem "compre agora", a menos que a sua própria chamada para ação diga. Uma edição que tire `{{brief_claims}}` ou `{{never_say}}` não deixa de enviá-los: o Bardo acrescenta as afirmações do briefing e o que nunca pode ser dito depois do prompt. A [checagem de afirmações](script.md#claims) que lê o roteiro depois não é um modelo.
- **Metadados de anúncio**: o post de cada rede de um anúncio, na etapa Publicação ([Antes de um anúncio ir ao ar](uploading.md#ad-review)). Além dos dados do canal, do tema, de `{{script}}` e de `{{networks}}`, ele lê `{{product}}`, `{{product_concept}}`, `{{ad_hook}}`, `{{brief_claims}}`, `{{offer}}`, `{{call_to_action}}` e `{{link}}` (para onde o anúncio leva o espectador, vindo da etapa Briefing). Depois das instruções dele, o Bardo sempre acrescenta as regras de todo anúncio e, num produto do tipo *ideia*, as regras de um conceito; as afirmações do briefing e o que nunca pode ser dito são sempre enviados, mesmo numa edição que tire `{{brief_claims}}`. A checagem dos textos do post que vem depois não é um modelo.
- **Plano de cenas de anúncio**: o plano de cenas de um anúncio ([As cenas de um anúncio](scenes.md#ad)), no lugar do modelo de prompts de imagem. Além do que aquele lê (os dados do canal, `{{visual_style}}`, o tema, `{{narration_sentences}}` e `{{frame_shape}}`), ele lê `{{product}}`, `{{product_concept}}`, `{{call_to_action}}`, `{{product_images}}` (as imagens do produto, numeradas a partir de 0, com o que cada uma é e o formato dela) e `{{scene_kinds}}` (os tipos que uma cena deste anúncio pode ter, conforme as imagens do produto). Depois das instruções dele, o Bardo sempre acrescenta as regras de todo anúncio e as regras das cenas de um anúncio: só os tipos listados, só a última cena como cartão final, nada do produto desenhado ou redesenhado numa foto do produto ou no cartão final, nenhuma outra marca, logo ou pessoa real numa imagem desenhada, e nenhum texto, legenda ou marca d'água na imagem além do nome do próprio produto nele. Uma edição que tire `{{product_images}}` ou `{{scene_kinds}}` não deixa de enviá-los: o Bardo acrescenta os dois depois do prompt.
- **Prompt de conceito**: o prompt de imagem da imagem de conceito de um produto do tipo *ideia* ([Uma imagem de conceito para uma ideia](products.md#concept)). Ele lê `{{product}}` e `{{product_audience}}`. Depois das instruções dele, e de novo depois do prompt, o Bardo sempre acrescenta uma instrução que nenhuma edição tira: desenhar um produto original, sem marca, logo ou identidade visual que já existam, e sem nenhum texto. O gerador de imagens recebe o mesmo aviso depois do prompt do Claude.

<a id="versions"></a>
## Versões

Cada vez que você salva, entra uma versão, numerada em ordem; a mais nova é a **Atual** e é a que as gerações usam. As versões anteriores continuam na lista: escolha uma para carregar o texto dela no editor, e salve para torná-la a atual de novo. Cada geração registra a versão que usou, então os **Detalhes** de um roteiro dizem qual versão o escreveu.

Quando uma atualização do Bardo melhora um padrão que você nunca mudou, o padrão novo entra como a próxima versão. Um modelo que você editou fica como você deixou; **Carregar o padrão do Bardo** mostra o que mudou.

<a id="fields"></a>
## Instruções e prompt

Um modelo tem duas partes, cada uma com até 8.000 caracteres:

- **Instruções**: as regras fixas, como o papel, o tom e o formato da resposta.
- **Prompt**: a tarefa em si, com os dados do projeto colocados por meio de variáveis. Ele não pode ficar vazio.

<a id="variables"></a>
## Variáveis

Escreva uma variável como `{{nome}}`; cada geração a troca pelo valor do projeto. A lista sob o editor mostra as variáveis que este tipo de modelo pode usar e o que cada uma traz, como `{{channel_name}}`, `{{persona}}` ou `{{narration_sentences}}`. `{{video_title}}` é o título do YouTube quando já foi escrito, senão o do projeto. `{{target_length}}` e `{{frame_shape}}` seguem o [formato](projects.md#format) do vídeo: o tamanho do roteiro (ou a [duração](projects.md#length) do projeto, quando ele escolhe uma) e o formato das imagens. Um modelo de roteiro sem `{{target_length}}` ignora os dois. `{{visual_style}}` (modelos de prompts de imagem e de miniatura) é o [estilo visual](channels.md#style) do vídeo: que um foi escolhido e as palavras que o Bardo acrescenta a cada imagem por ele, ou que nenhum foi escolhido. O Bardo acrescenta o estilo escolhido às imagens, diga o modelo o que disser. Um modelo cuja parte de prompt você editou e que não tem a variável nunca conta ao Claude que um estilo foi escolhido, então o Claude continua escrevendo o visual das notas em cada prompt: inclua nele uma linha com `{{visual_style}}`. `{{fixed_cast}}` (modelos de prompts de imagem) traz os personagens escolhidos em [Quem aparece](scenes.md#who-appears), cada um com a aparência, as notas, os pertences e a instrução deste vídeo; `{{fixed_places}}` e `{{fixed_objects}}` trazem do mesmo jeito os lugares e objetos escolhidos. Um modelo que não coloca uma delas a recebe depois do prompt. Uma variável escrita errado ou desconhecida, ou sem o `}}` de fechamento, impede que o modelo seja salvo.

<a id="save"></a>
## Salvar e recomeçar

**Salvar como nova versão** guarda o seu texto como a próxima versão; quando nada mudou, nenhuma versão é criada. **Descartar alterações** volta à versão atual. **Carregar o padrão do Bardo** põe no editor o texto com que o Bardo veio, para salvar como nova versão se você quiser de volta.
